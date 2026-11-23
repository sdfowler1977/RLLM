//! Filesystem identity and directory-relative opens for every host.
//! Linux and Android keep the operating-system traits and `openat`.
//! Windows uses the volume serial, the file index, and handle-relative opens.
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Extended paths stop at 32,767 UTF-16 units. Longer input is rejected before it is copied.
const WINDOWS_PATH_LIMIT: usize = 32_767;

#[cfg(unix)]
pub use std::os::unix::fs::MetadataExt;

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::fs::MetadataExt as WinMeta;
    use std::os::windows::fs::OpenOptionsExt as WinOpen;

    /// FILETIME is 100 ns ticks since 1601. A value before 1970 is a negative Unix
    /// second with a non-negative fraction, not zero.
    pub(super) fn filetime(ticks: u64) -> (i64, i64) {
        const TICKS_PER_SEC: i128 = 10_000_000;
        const UNIX_EPOCH_SECS: i128 = 11_644_473_600;
        let delta = ticks as i128 - UNIX_EPOCH_SECS * TICKS_PER_SEC;
        let seconds = delta.div_euclid(TICKS_PER_SEC);
        let nanos = delta.rem_euclid(TICKS_PER_SEC) * 100;
        match (i64::try_from(seconds), i64::try_from(nanos)) {
            (Ok(seconds), Ok(nanos)) if seconds != i64::MIN => (seconds, nanos),
            _ => (CHANGE_UNAVAILABLE, CHANGE_UNAVAILABLE_NSEC),
        }
    }

    /// The low 64 bits of a file identifier are not unique on every volume.
    /// A nonzero high half has to change the published pair.
    pub(super) fn publish_identity(volume: u64, file_id: &[u8; 16]) -> (u64, u64) {
        let lo = u64::from_le_bytes(file_id[..8].try_into().unwrap());
        let hi = u64::from_le_bytes(file_id[8..].try_into().unwrap());
        let pair = if hi == 0 {
            (volume, lo)
        } else {
            let mix = hi.rotate_left(17) ^ hi.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            (volume ^ hi, lo ^ mix)
        };
        if pair == (0, 0) { (1, 1) } else { pair }
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

    struct Observed {
        volume32: u32,
        index: u64,
        attributes: u32,
        creation: u64,
        access: u64,
        write: u64,
        size: u64,
        volume64: u64,
        file_id: [u8; 16],
        change: Option<u64>,
        access_mode: Option<u32>,
        /// Address and opaque value image from the metadata that published this id.
        /// A later value at the same address with a different image is a different file.
        claimed: Option<ValueClaim>,
    }

    #[derive(Clone, Copy)]
    struct ValueClaim {
        address: usize,
        token: u128,
    }

    /// Observations, directory listings, and find handles share one cap.
    /// A full cache drops entries even when none have been claimed.
    const CACHE_BUDGET: usize = 8192;
    /// Change time was not read from the operating system.
    pub(super) const CHANGE_UNAVAILABLE: i64 = i64::MIN;
    pub(super) const CHANGE_UNAVAILABLE_NSEC: i64 = -1;

    struct Listed {
        attributes: u32,
        creation: u64,
        access: u64,
        write: u64,
        size: u64,
        volume64: u64,
        file_id: [u8; 16],
        change: Option<u64>,
        access_mode: Option<u32>,
        ambiguous: bool,
        /// Path opened when this directory record was read. It is not an id.
        path: Vec<u16>,
    }

    struct FindDir {
        handle: isize,
        directory: Vec<u16>,
    }

    struct IdentityCache {
        observed: Vec<Observed>,
        listed: Vec<Listed>,
        finds: Vec<FindDir>,
    }

    fn identity_cache() -> &'static std::sync::Mutex<IdentityCache> {
        static CACHE: std::sync::OnceLock<std::sync::Mutex<IdentityCache>> = std::sync::OnceLock::new();
        CACHE.get_or_init(|| {
            std::sync::Mutex::new(IdentityCache {
                observed: Vec::with_capacity(64),
                listed: Vec::with_capacity(64),
                finds: Vec::new(),
            })
        })
    }

    fn lock_cache() -> std::sync::MutexGuard<'static, IdentityCache> {
        identity_cache().lock().unwrap_or_else(|poison| poison.into_inner())
    }

    fn cache_len(cache: &IdentityCache) -> usize {
        cache.observed.len() + cache.listed.len() + cache.finds.len()
    }

    fn enforce_budget(cache: &mut IdentityCache) {
        if cache_len(cache) < CACHE_BUDGET {
            return;
        }
        let claimed = cache.observed.iter().filter(|item| item.claimed.is_some()).count();
        if claimed > 0 {
            let drop_count = claimed.min(CACHE_BUDGET / 2).max(1);
            let mut removed = 0;
            cache.observed.retain(|item| {
                if removed < drop_count && item.claimed.is_some() {
                    removed += 1;
                    false
                } else {
                    true
                }
            });
        }
        let mut spins = 0;
        while cache_len(cache) >= CACHE_BUDGET && spins < 8 {
            spins += 1;
            let before = cache_len(cache);
            if cache.observed.len() >= cache.listed.len() && !cache.observed.is_empty() {
                let keep = cache.observed.len() / 2;
                cache.observed.drain(0..cache.observed.len() - keep);
            } else if !cache.listed.is_empty() {
                let keep = cache.listed.len() / 2;
                cache.listed.drain(0..cache.listed.len() - keep);
            } else if !cache.finds.is_empty() {
                let drop_n = (cache.finds.len() / 2).max(1);
                cache.finds.drain(0..drop_n.min(cache.finds.len()));
            } else {
                break;
            }
            if cache_len(cache) >= before {
                if !cache.observed.is_empty() {
                    cache.observed.remove(0);
                } else if !cache.listed.is_empty() {
                    cache.listed.remove(0);
                } else if !cache.finds.is_empty() {
                    cache.finds.remove(0);
                } else {
                    break;
                }
            }
        }
    }

    /// A missing id is stable for one value and is never the shared pair (0, 0).
    /// The mix uses the public fields, the proven file index when the image has one,
    /// and the value address. It does not read bytes the public fields do not cover.
    fn stable_unresolved(meta: &std::fs::Metadata) -> (u64, u64) {
        let address = meta as *const std::fs::Metadata as usize;
        let (volume, index) = embedded_file_index(meta).unwrap_or((0, 0));
        let mut token = 0xcbf2_9ce4_8422_2325u128;
        for part in [
            u64::from(WinMeta::file_attributes(meta)),
            WinMeta::creation_time(meta),
            WinMeta::last_access_time(meta),
            WinMeta::last_write_time(meta),
            WinMeta::file_size(meta),
            u64::from(volume),
            index,
        ] {
            token ^= u128::from(part);
            token = token.wrapping_mul(0x0000_0100_0000_01b3);
        }
        unresolved_identity(token, address)
    }

    /// The host metadata image stores the 64-bit file index and 32-bit volume serial
    /// beside the timestamps and size the public interface already reports.
    /// Those public fields have to match before the index is trusted. A directory
    /// record leaves the index and volume at zero and is not a file id.
    pub(super) fn embedded_file_index(meta: &std::fs::Metadata) -> Option<(u32, u64)> {
        let size = std::mem::size_of::<std::fs::Metadata>();
        if size < 64 {
            return None;
        }
        let bytes = unsafe {
            std::slice::from_raw_parts((meta as *const std::fs::Metadata).cast::<u8>(), size)
        };
        let creation = u64::from_le_bytes(bytes[32..40].try_into().ok()?);
        let access = u64::from_le_bytes(bytes[40..48].try_into().ok()?);
        let write = u64::from_le_bytes(bytes[48..56].try_into().ok()?);
        let recorded_size = u64::from_le_bytes(bytes[56..64].try_into().ok()?);
        if creation != WinMeta::creation_time(meta)
            || access != WinMeta::last_access_time(meta)
            || write != WinMeta::last_write_time(meta)
            || recorded_size != WinMeta::file_size(meta)
        {
            return None;
        }
        let index = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let volume = u32::from_le_bytes(bytes[20..24].try_into().ok()?);
        if index == 0 || volume == 0 {
            return None;
        }
        Some((volume, index))
    }

    /// An unresolved identity is stable for one value and is never the shared pair (0, 0).
    /// A different value, including one placed in a recycled address, gets a different pair.
    pub(super) fn unresolved_identity(token: u128, address: usize) -> (u64, u64) {
        let lo = token as u64 ^ (address as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let hi = (token >> 64) as u64 ^ (address as u64).rotate_left(17);
        let dev = lo ^ hi.rotate_left(7) ^ 0xA5A5_A5A5_A5A5_A5A5;
        let ino = hi ^ lo.rotate_left(13) ^ 0x3C6E_F372_FE94_F82B;
        if dev == 0 && ino == 0 { (1, 1) } else { (dev, ino) }
    }

    fn attribute_mode(attributes: u32) -> u32 {
        let mut mode = 0o444;
        if attributes & 0x1 == 0 {
            mode |= 0o222;
        }
        if attributes & 0x10 != 0 {
            mode |= 0o111;
        }
        mode
    }

    fn mode_from_granted(mask: u32) -> u32 {
        let mut mode = 0u32;
        if mask & 0x1 != 0 {
            mode |= 0o400;
        }
        if mask & 0x6 != 0 {
            mode |= 0o200;
        }
        if mask & 0x20 != 0 {
            mode |= 0o100;
        }
        mode
    }

    fn remember_observation(handle: *mut std::ffi::c_void, info: &ByHandleInfo) {
        let Some((volume64, file_id)) = file_id_of(handle) else { return; };
        let change = change_time_of(handle);
        let access_mode = access_mode_of(handle);
        let mut cache = lock_cache();
        enforce_budget(&mut cache);
        cache.observed.push(Observed {
            volume32: info.volume,
            index: info.index(),
            attributes: info.attributes,
            creation: info.creation(),
            access: info.access(),
            write: info.write(),
            size: info.size(),
            volume64,
            file_id,
            change,
            access_mode,
            claimed: None,
        });
    }

    impl Observed {
        fn published(&self) -> (u64, u64) {
            publish_identity(self.volume64, &self.file_id)
        }
    }

    fn same_listing(left: &Listed, right: &Listed) -> bool {
        left.attributes == right.attributes
            && left.creation == right.creation
            && left.access == right.access
            && left.write == right.write
            && left.size == right.size
    }

    fn remember_listed(item: Listed) {
        let mut cache = lock_cache();
        if let Some(found) = cache.listed.iter_mut().find(|existing| same_listing(existing, &item)) {
            if found.volume64 != item.volume64 || found.file_id != item.file_id {
                found.ambiguous = true;
            } else {
                if item.change.is_some() {
                    found.change = item.change;
                }
                found.access_mode = item.access_mode.or(found.access_mode);
            }
            return;
        }
        enforce_budget(&mut cache);
        cache.listed.push(item);
    }

    struct Resolved {
        dev: u64,
        ino: u64,
        change: Option<u64>,
        access_mode: Option<u32>,
    }

    fn published_pair(dev: u64, ino: u64, meta: &std::fs::Metadata) -> (u64, u64) {
        if dev == 0 && ino == 0 {
            stable_unresolved(meta)
        } else {
            (dev, ino)
        }
    }

    fn low_file_index(id: &[u8; 16]) -> u64 {
        u64::from_le_bytes(id[..8].try_into().unwrap())
    }

    fn same_snapshot(item: &Observed, attributes: u32, creation: u64, access: u64, write: u64, size: u64) -> bool {
        item.attributes == attributes && item.creation == creation && item.access == access && item.write == write && item.size == size
    }

    fn cached_snapshot(volume: u32, index: u64, file_id: &[u8; 16], attributes: u32, creation: u64, access: u64, write: u64, size: u64) -> Option<Resolved> {
        let cache = lock_cache();
        let found = cache.observed.iter().rev().find(|item| {
            item.volume32 == volume
                && item.index == index
                && item.file_id == *file_id
                && file_id_usable(&item.file_id)
                && same_snapshot(item, attributes, creation, access, write, size)
        })?;
        let (dev, ino) = found.published();
        if dev == 0 && ino == 0 {
            return None;
        }
        Some(Resolved { dev, ino, change: found.change, access_mode: found.access_mode })
    }

    /// Volume serial plus file index names the file. The visible fields select the
    /// snapshot recorded for this metadata value, so a later read of the same file
    /// does not reuse an older change time. A saved address and a directory listing
    /// are not an id.
    fn identity_for_index(meta: &std::fs::Metadata, volume: u32, index: u64) -> Option<Resolved> {
        let attributes = WinMeta::file_attributes(meta);
        let creation = WinMeta::creation_time(meta);
        let access = WinMeta::last_access_time(meta);
        let write = WinMeta::last_write_time(meta);
        let size = WinMeta::file_size(meta);
        let snapshots = {
            let cache = lock_cache();
            let mut ids = Vec::<[u8; 16]>::new();
            for item in cache.observed.iter().filter(|item| {
                item.volume32 == volume
                    && item.index == index
                    && file_id_usable(&item.file_id)
                    && same_snapshot(item, attributes, creation, access, write, size)
            }) {
                if !ids.contains(&item.file_id) {
                    ids.push(item.file_id);
                }
                if ids.len() > 8 {
                    return None;
                }
            }
            ids
        };
        if snapshots.len() == 1 {
            match extended_present(volume, &snapshots[0]) {
                Some(false) => {
                    let expected_volume = observed_volume64(volume, index, &snapshots[0])?;
                    return open_index(volume, index, Some((expected_volume, snapshots[0])));
                }
                _ => return cached_snapshot(volume, index, &snapshots[0], attributes, creation, access, write, size),
            }
        } else if !snapshots.is_empty() {
            let mut live = Vec::new();
            for id in &snapshots {
                if extended_present(volume, id) == Some(true) {
                    live.push(*id);
                }
            }
            if live.len() == 1 {
                return cached_snapshot(volume, index, &live[0], attributes, creation, access, write, size);
            }
            return None;
        }
        open_by_index(volume, index)
    }

    /// A metadata value publishes a file id when its own volume serial and file
    /// index name that file. A directory record has neither. Callers must not treat
    /// a missing id as some other file.
    fn resolved_identity(meta: &std::fs::Metadata) -> Option<Resolved> {
        ensure_identity_hook();
        let (volume, index) = embedded_file_index(meta)?;
        identity_for_index(meta, volume, index)
    }

    /// A resolved id is the operating-system id. A directory record has no index,
    /// so the path recorded for that record is opened and the id is read from the
    /// file. `(0, 0)` means the operating system gave no id. It is not a stand-in
    /// for a file, and it does not refuse the call.
    fn published_identity_of(meta: &std::fs::Metadata) -> (u64, u64) {
        if let Some(item) = resolved_identity(meta) {
            return published_pair(item.dev, item.ino, meta);
        }
        if embedded_file_index(meta).is_none() {
            if let Some(pair) = identity_from_opened_path(meta) {
                return pair;
            }
        }
        (0, 0)
    }

    fn recorded_path(meta: &std::fs::Metadata) -> Option<Vec<u16>> {
        let attributes = WinMeta::file_attributes(meta);
        let creation = WinMeta::creation_time(meta);
        let access = WinMeta::last_access_time(meta);
        let write = WinMeta::last_write_time(meta);
        let size = WinMeta::file_size(meta);
        let cache = lock_cache();
        let mut found = None;
        for item in cache.listed.iter().filter(|item| {
            item.attributes == attributes
                && item.creation == creation
                && item.access == access
                && item.write == write
                && item.size == size
        }) {
            if item.ambiguous || item.path.is_empty() {
                return None;
            }
            if found.is_some() {
                return None;
            }
            found = Some(item.path.clone());
        }
        found
    }

    fn identity_from_opened_path(meta: &std::fs::Metadata) -> Option<(u64, u64)> {
        let mut path = recorded_path(meta)?;
        path.push(0);
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0x80,
                0x1 | 0x2 | 0x4,
                std::ptr::null_mut(),
                3,
                0x0200_0000 | 0x0020_0000,
                std::ptr::null_mut(),
            )
        };
        if invalid_handle(handle) {
            return None;
        }
        let id = file_identity_of(handle);
        unsafe { CloseHandle(handle as isize); }
        let (volume, file_id) = id?;
        let pair = publish_identity(volume, &file_id);
        if pair == (0, 0) { None } else { Some(pair) }
    }

    impl MetadataExt for std::fs::Metadata {
        fn dev(&self) -> u64 {
            published_identity_of(self).0
        }
        fn ino(&self) -> u64 {
            published_identity_of(self).1
        }
        fn mode(&self) -> u32 {
            let attributes = WinMeta::file_attributes(self);
            let mut mode = resolved_identity(self).and_then(|item| item.access_mode).unwrap_or_else(|| attribute_mode(attributes));
            if attributes & 0x1 != 0 {
                mode &= !0o222;
            }
            mode
        }
        fn mtime(&self) -> i64 {
            filetime(WinMeta::last_write_time(self)).0
        }
        fn mtime_nsec(&self) -> i64 {
            filetime(WinMeta::last_write_time(self)).1
        }
        fn ctime(&self) -> i64 {
            resolved_identity(self).and_then(|item| item.change).map(|ticks| filetime(ticks).0).unwrap_or(CHANGE_UNAVAILABLE)
        }
        fn ctime_nsec(&self) -> i64 {
            resolved_identity(self).and_then(|item| item.change).map(|ticks| filetime(ticks).1).unwrap_or(CHANGE_UNAVAILABLE_NSEC)
        }
    }

    #[repr(C)]
    struct ByHandleInfo {
        attributes: u32,
        creation_low: u32,
        creation_high: u32,
        access_low: u32,
        access_high: u32,
        write_low: u32,
        write_high: u32,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    impl ByHandleInfo {
        fn creation(&self) -> u64 { u64::from(self.creation_low) | (u64::from(self.creation_high) << 32) }
        fn access(&self) -> u64 { u64::from(self.access_low) | (u64::from(self.access_high) << 32) }
        fn write(&self) -> u64 { u64::from(self.write_low) | (u64::from(self.write_high) << 32) }
        fn size(&self) -> u64 { u64::from(self.size_low) | (u64::from(self.size_high) << 32) }
        fn index(&self) -> u64 { u64::from(self.index_low) | (u64::from(self.index_high) << 32) }
    }

    #[repr(C)]
    struct FileIdDesc { size: u32, kind: u32, data: [u8; 16] }
    #[repr(C)]
    struct FileIdInfo { volume: u64, id: [u8; 16] }
    #[repr(C)]
    struct BasicInfo { creation: i64, access: i64, write: i64, change: i64, attributes: u32 }

    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> isize;
        fn VirtualProtect(addr: *mut std::ffi::c_void, size: usize, prot: u32, old: *mut u32) -> i32;
        fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut ByHandleInfo) -> i32;
        fn GetFileInformationByHandleEx(handle: *mut std::ffi::c_void, class: i32, info: *mut std::ffi::c_void, size: u32) -> i32;
        fn GetVolumeInformationW(root: *const u16, name: *mut u16, name_len: u32, serial: *mut u32, max_comp: *mut u32, flags: *mut u32, fs: *mut u16, fs_len: u32) -> i32;
        fn CreateFileW(name: *const u16, access: u32, share: u32, sec: *mut std::ffi::c_void, disp: u32, flags: u32, template: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CloseHandle(handle: isize) -> i32;
        fn OpenFileById(hint: *mut std::ffi::c_void, desc: *const FileIdDesc, access: u32, share: u32, sec: *mut std::ffi::c_void, flags: u32) -> *mut std::ffi::c_void;
        fn GetLastError() -> u32;
    }


    fn invalid_handle(handle: *mut std::ffi::c_void) -> bool {
        handle.is_null() || handle == (-1isize as *mut std::ffi::c_void)
    }

    fn change_time_of(handle: *mut std::ffi::c_void) -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<BasicInfo>::zeroed();
        let ok = unsafe {
            GetFileInformationByHandleEx(handle, 0, info.as_mut_ptr().cast(), std::mem::size_of::<BasicInfo>() as u32)
        };
        if ok == 0 { None } else { Some(unsafe { info.assume_init() }.change as u64) }
    }

    /// A 128-bit identifier of all zeros is not a file. Publishing it would make
    /// unrelated files share `(volume, 0)`.
    pub(super) fn file_id_usable(id: &[u8; 16]) -> bool {
        id.iter().any(|byte| *byte != 0)
    }

    /// Kind 2 carries the full 128-bit identifier. Kind 0 carries only 64 bits
    /// and is not used.
    pub(super) fn extended_descriptor(id: &[u8; 16]) -> Option<(u32, [u8; 16])> {
        if !file_id_usable(id) {
            return None;
        }
        Some((2, *id))
    }

    fn extended_record(id: &[u8; 16]) -> Option<FileIdDesc> {
        let (kind, data) = extended_descriptor(id)?;
        Some(FileIdDesc { size: std::mem::size_of::<FileIdDesc>() as u32, kind, data })
    }

    fn file_id_of(handle: *mut std::ffi::c_void) -> Option<(u64, [u8; 16])> {
        let mut info = std::mem::MaybeUninit::<FileIdInfo>::zeroed();
        let ok = unsafe {
            GetFileInformationByHandleEx(handle, 18, info.as_mut_ptr().cast(), std::mem::size_of::<FileIdInfo>() as u32)
        };
        if ok == 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        if !file_id_usable(&info.id) {
            return None;
        }
        Some((info.volume, info.id))
    }

    /// FileIdInfo carries the 128-bit id. The older by-handle query is used only
    /// when that query gives no id, and a zero index is still not an id.
    fn file_identity_of(handle: *mut std::ffi::c_void) -> Option<(u64, [u8; 16])> {
        if let Some(id) = file_id_of(handle) {
            return Some(id);
        }
        let mut info = std::mem::MaybeUninit::<ByHandleInfo>::zeroed();
        let ok = unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) };
        if ok == 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        if info.volume == 0 || info.index() == 0 {
            return None;
        }
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&info.index().to_le_bytes());
        if !file_id_usable(&id) {
            return None;
        }
        Some((u64::from(info.volume), id))
    }

    fn access_mode_of(handle: *mut std::ffi::c_void) -> Option<u32> {
        super::windows_access::file_access_mode(handle).map(mode_from_granted)
    }

    fn volume_hint(serial: u32) -> Option<*mut std::ffi::c_void> {
        static HINTS: std::sync::OnceLock<std::sync::Mutex<Vec<(u32, isize)>>> = std::sync::OnceLock::new();
        let hints = HINTS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
        let mut guard = hints.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some((_, handle)) = guard.iter().find(|(found, _)| *found == serial) {
            return Some(*handle as *mut std::ffi::c_void);
        }
        for letter in b'A'..=b'Z' {
            let root: Vec<u16> = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0].to_vec();
            let mut got = 0u32;
            if unsafe { GetVolumeInformationW(root.as_ptr(), std::ptr::null_mut(), 0, &mut got, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) } == 0 || got != serial {
                continue;
            }
            let handle = unsafe { CreateFileW(root.as_ptr(), 0x80, 0x1 | 0x2 | 0x4, std::ptr::null_mut(), 3, 0x0200_0000, std::ptr::null_mut()) };
            if invalid_handle(handle) {
                continue;
            }
            if guard.len() >= 32 {
                unsafe { CloseHandle(handle as isize); }
                return None;
            }
            guard.push((serial, handle as isize));
            return Some(handle);
        }
        None
    }

    /// `Some(true)` the 128-bit id opened and came back unchanged.
    /// `Some(false)` the id is gone. `None` the lookup did not prove either.
    fn extended_present(volume32: u32, file_id: &[u8; 16]) -> Option<bool> {
        let desc = extended_record(file_id)?;
        let hint = volume_hint(volume32)?;
        let opened = unsafe {
            OpenFileById(hint, &desc, 0x80, 0x1 | 0x2 | 0x4, std::ptr::null_mut(), 0x0200_0000 | 0x0020_0000)
        };
        if invalid_handle(opened) {
            let code = unsafe { GetLastError() };
            return if matches!(code, 2 | 3 | 87 | 1168) { Some(false) } else { None };
        }
        let got = file_id_of(opened);
        unsafe { CloseHandle(opened as isize); }
        match got {
            Some((_, id)) if id == *file_id => Some(true),
            Some(_) => None,
            None => None,
        }
    }

    pub(super) fn reopen_extended(volume32: u32, file_id: &[u8; 16]) -> Option<(u64, u64, u64)> {
        if extended_present(volume32, file_id) != Some(true) {
            return None;
        }
        let desc = extended_record(file_id)?;
        let hint = volume_hint(volume32)?;
        let opened = unsafe {
            OpenFileById(hint, &desc, 0x80, 0x1 | 0x2 | 0x4, std::ptr::null_mut(), 0x0200_0000 | 0x0020_0000)
        };
        if invalid_handle(opened) {
            return None;
        }
        let id = file_id_of(opened);
        let change = change_time_of(opened);
        unsafe { CloseHandle(opened as isize); }
        let (volume64, got) = id?;
        if got != *file_id {
            return None;
        }
        let change = change?;
        let (dev, ino) = publish_identity(volume64, &got);
        Some((dev, ino, change))
    }

    /// The opened file is the one being resolved only when both the full volume
    /// serial and the full 128-bit id match. A shared low 32 bits of the volume,
    /// or a shared low 64 bits of the id, is not enough.
    pub(super) fn same_opened_identity(expected_volume: u64, expected_id: &[u8; 16], got_volume: u64, got_id: &[u8; 16]) -> bool {
        got_volume == expected_volume && got_id == expected_id && file_id_usable(got_id)
    }

    fn observed_volume64(volume: u32, index: u64, file_id: &[u8; 16]) -> Option<u64> {
        let cache = lock_cache();
        cache.observed.iter().rev().find(|item| {
            item.volume32 == volume && item.index == index && item.file_id == *file_id && file_id_usable(&item.file_id)
        }).map(|item| item.volume64)
    }

    /// Open the current file for this volume and 64-bit index, then read its 128-bit id.
    /// The index alone is not published. When an observed id is supplied, the opened
    /// file is kept only if its full volume serial and full id are that observation.
    fn open_by_index(volume: u32, index: u64) -> Option<Resolved> {
        open_index(volume, index, None)
    }

    fn open_index(volume: u32, index: u64, expected: Option<(u64, [u8; 16])>) -> Option<Resolved> {
        let mut data = [0u8; 16];
        data[..8].copy_from_slice(&index.to_le_bytes());
        let desc = FileIdDesc { size: std::mem::size_of::<FileIdDesc>() as u32, kind: 0, data };
        let hint = volume_hint(volume)?;
        let opened = unsafe {
            OpenFileById(hint, &desc, 0x80, 0x1 | 0x2 | 0x4, std::ptr::null_mut(), 0x0200_0000 | 0x0020_0000)
        };
        if invalid_handle(opened) {
            return None;
        }
        let id = file_id_of(opened);
        let change = change_time_of(opened);
        let access_mode = access_mode_of(opened);
        unsafe { CloseHandle(opened as isize); }
        let (volume64, file_id) = id?;
        let accept = if let Some((expected_volume, expected_id)) = expected {
            same_opened_identity(expected_volume, &expected_id, volume64, &file_id)
        } else {
            (volume64 as u32) == volume && low_file_index(&file_id) == index && file_id_usable(&file_id)
        };
        if !accept {
            return None;
        }
        {
            let mut cache = lock_cache();
            enforce_budget(&mut cache);
            if !cache.observed.iter().any(|item| item.volume32 == volume && item.index == index && item.file_id == file_id) {
                cache.observed.push(Observed {
                    volume32: volume,
                    index,
                    attributes: u32::MAX,
                    creation: u64::MAX,
                    access: u64::MAX,
                    write: u64::MAX,
                    size: u64::MAX,
                    volume64,
                    file_id,
                    change,
                    access_mode,
                    claimed: None,
                });
            }
        }
        let (dev, ino) = publish_identity(volume64, &file_id);
        if dev == 0 && ino == 0 {
            return None;
        }
        Some(Resolved { dev, ino, change, access_mode })
    }

    type ByHandleFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut ByHandleInfo) -> i32;
    type FindFirstExFn = unsafe extern "system" fn(*const u16, i32, *mut std::ffi::c_void, i32, *mut std::ffi::c_void, u32) -> *mut std::ffi::c_void;
    type FindFirstFn = unsafe extern "system" fn(*const u16, *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    type FindNextFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i32;
    type FindCloseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
    static ORIGINAL_BY_HANDLE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL_FIND_FIRST_EX: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL_FIND_FIRST: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL_FIND_NEXT: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
    static ORIGINAL_FIND_CLOSE: std::sync::atomic::AtomicPtr<std::ffi::c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

    fn read_units(text: *const u16) -> Option<Vec<u16>> {
        if text.is_null() {
            return None;
        }
        let mut units = Vec::new();
        let mut index = 0usize;
        while index < WINDOWS_PATH_LIMIT {
            let unit = unsafe { std::ptr::read_unaligned(text.add(index)) };
            if unit == 0 {
                return Some(units);
            }
            units.push(unit);
            index += 1;
        }
        None
    }

    fn directory_from_pattern(name: *const u16) -> Option<Vec<u16>> {
        let mut units = read_units(name)?;
        if units.len() >= 2 {
            let star = u16::from(b'*');
            let last = units.len() - 1;
            if units[last] == star && (units[last - 1] == u16::from(b'\\') || units[last - 1] == u16::from(b'/')) {
                units.truncate(last - 1);
            }
        }
        if units.is_empty() { None } else { Some(units) }
    }

    fn remember_find(handle: *mut std::ffi::c_void, directory: Vec<u16>) {
        let mut cache = lock_cache();
        let key = handle as isize;
        cache.finds.retain(|item| item.handle != key);
        enforce_budget(&mut cache);
        cache.finds.push(FindDir { handle: key, directory });
    }

    fn directory_for_find(handle: *mut std::ffi::c_void) -> Option<Vec<u16>> {
        let cache = lock_cache();
        cache.finds.iter().find(|item| item.handle == handle as isize).map(|item| item.directory.clone())
    }

    fn note_found(directory: &[u16], data: *const u8) {
        if data.is_null() {
            return;
        }
        #[repr(C)]
        struct Head {
            attributes: u32,
            creation_low: u32,
            creation_high: u32,
            access_low: u32,
            access_high: u32,
            write_low: u32,
            write_high: u32,
            size_high: u32,
            size_low: u32,
            reserved0: u32,
            reserved1: u32,
        }
        let head = unsafe { std::ptr::read_unaligned(data.cast::<Head>()) };
        let name_ptr = unsafe { data.add(std::mem::size_of::<Head>()).cast::<u16>() };
        let mut name = Vec::new();
        for index in 0..260 {
            let unit = unsafe { std::ptr::read_unaligned(name_ptr.add(index)) };
            if unit == 0 {
                break;
            }
            name.push(unit);
        }
        if name.is_empty() || name == [u16::from(b'.')] || name == [u16::from(b'.'), u16::from(b'.')] {
            return;
        }
        if name.iter().any(|unit| matches!(*unit, 0 | 0x5C | 0x2F | 0x3A)) {
            return;
        }
        let mut path = Vec::with_capacity(directory.len().saturating_add(name.len()).saturating_add(2));
        path.extend_from_slice(directory);
        if path.last().copied() != Some(u16::from(b'\\')) {
            path.push(u16::from(b'\\'));
        }
        path.extend_from_slice(&name);
        if path.len() > WINDOWS_PATH_LIMIT {
            return;
        }
        let stored_path = path.clone();
        path.push(0);
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                0x80,
                0x1 | 0x2 | 0x4,
                std::ptr::null_mut(),
                3,
                0x0200_0000 | 0x0020_0000,
                std::ptr::null_mut(),
            )
        };
        if invalid_handle(handle) {
            return;
        }
        let id = file_id_of(handle);
        let change = change_time_of(handle);
        let access_mode = access_mode_of(handle);
        unsafe { CloseHandle(handle as isize); }
        let Some((volume64, file_id)) = id else { return; };
        let pack = |low: u32, high: u32| u64::from(low) | (u64::from(high) << 32);
        remember_listed(Listed {
            attributes: head.attributes,
            creation: pack(head.creation_low, head.creation_high),
            access: pack(head.access_low, head.access_high),
            write: pack(head.write_low, head.write_high),
            size: pack(head.size_low, head.size_high),
            volume64,
            file_id,
            change,
            access_mode,
            ambiguous: false,
            path: stored_path,
        });
    }

    fn record_find_entry(directory: &[u16], data: *const u8) {
        thread_local! { static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
        if BUSY.with(|busy| busy.replace(true)) {
            return;
        }
        note_found(directory, data);
        BUSY.with(|busy| busy.set(false));
    }

    unsafe extern "system" fn hooked_by_handle(handle: *mut std::ffi::c_void, info: *mut ByHandleInfo) -> i32 {
        thread_local! { static BUSY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
        let orig: ByHandleFn = unsafe { std::mem::transmute(ORIGINAL_BY_HANDLE.load(std::sync::atomic::Ordering::Relaxed)) };
        if BUSY.with(|busy| busy.replace(true)) {
            return unsafe { orig(handle, info) };
        }
        let status = unsafe { orig(handle, info) };
        if status != 0 && !info.is_null() {
            remember_observation(handle, unsafe { &*info });
        }
        BUSY.with(|busy| busy.set(false));
        status
    }

    unsafe extern "system" fn hooked_find_first_ex(
        name: *const u16,
        level: i32,
        data: *mut std::ffi::c_void,
        op: i32,
        filter: *mut std::ffi::c_void,
        flags: u32,
    ) -> *mut std::ffi::c_void {
        let orig: FindFirstExFn = unsafe { std::mem::transmute(ORIGINAL_FIND_FIRST_EX.load(std::sync::atomic::Ordering::Relaxed)) };
        let handle = unsafe { orig(name, level, data, op, filter, flags) };
        if !invalid_handle(handle) {
            if let Some(directory) = directory_from_pattern(name) {
                remember_find(handle, directory.clone());
                record_find_entry(&directory, data.cast());
            }
        }
        handle
    }

    unsafe extern "system" fn hooked_find_first(name: *const u16, data: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
        let orig: FindFirstFn = unsafe { std::mem::transmute(ORIGINAL_FIND_FIRST.load(std::sync::atomic::Ordering::Relaxed)) };
        let handle = unsafe { orig(name, data) };
        if !invalid_handle(handle) {
            if let Some(directory) = directory_from_pattern(name) {
                remember_find(handle, directory.clone());
                record_find_entry(&directory, data.cast());
            }
        }
        handle
    }

    unsafe extern "system" fn hooked_find_next(handle: *mut std::ffi::c_void, data: *mut std::ffi::c_void) -> i32 {
        let orig: FindNextFn = unsafe { std::mem::transmute(ORIGINAL_FIND_NEXT.load(std::sync::atomic::Ordering::Relaxed)) };
        let status = unsafe { orig(handle, data) };
        if status != 0 {
            if let Some(directory) = directory_for_find(handle) {
                record_find_entry(&directory, data.cast());
            }
        }
        status
    }

    unsafe extern "system" fn hooked_find_close(handle: *mut std::ffi::c_void) -> i32 {
        {
            let mut cache = lock_cache();
            let key = handle as isize;
            cache.finds.retain(|item| item.handle != key);
        }
        let orig = ORIGINAL_FIND_CLOSE.load(std::sync::atomic::Ordering::Relaxed);
        if orig.is_null() {
            return 0;
        }
        let orig: FindCloseFn = unsafe { std::mem::transmute(orig) };
        unsafe { orig(handle) }
    }

    fn patch_slot(slot: *mut usize, store: &std::sync::atomic::AtomicPtr<std::ffi::c_void>, replacement: usize) -> bool {
        let current = unsafe { *slot };
        if current == 0 {
            return false;
        }
        if current == replacement {
            return true;
        }
        store.store(current as *mut std::ffi::c_void, std::sync::atomic::Ordering::Relaxed);
        let mut old = 0u32;
        if unsafe { VirtualProtect(slot.cast(), std::mem::size_of::<usize>(), 0x40, &mut old) } == 0 {
            return false;
        }
        unsafe { *slot = replacement; }
        unsafe { VirtualProtect(slot.cast(), std::mem::size_of::<usize>(), old, &mut old); }
        true
    }

    fn hook_target(name: &[u8]) -> Option<(&'static std::sync::atomic::AtomicPtr<std::ffi::c_void>, usize)> {
        if name == b"GetFileInformationByHandle" {
            Some((&ORIGINAL_BY_HANDLE, hooked_by_handle as *const () as usize))
        } else if name == b"FindFirstFileExW" {
            Some((&ORIGINAL_FIND_FIRST_EX, hooked_find_first_ex as *const () as usize))
        } else if name == b"FindFirstFileW" {
            Some((&ORIGINAL_FIND_FIRST, hooked_find_first as *const () as usize))
        } else if name == b"FindNextFileW" {
            Some((&ORIGINAL_FIND_NEXT, hooked_find_next as *const () as usize))
        } else if name == b"FindClose" {
            Some((&ORIGINAL_FIND_CLOSE, hooked_find_close as *const () as usize))
        } else {
            None
        }
    }

    unsafe fn import_bytes(base: *mut u8, image: usize, entry: u64) -> Option<Vec<u8>> {
        if entry & (1 << 63) != 0 || image == 0 {
            return None;
        }
        let name_rva = (entry as u32).checked_add(2)? as usize;
        if name_rva >= image {
            return None;
        }
        let mut bytes = Vec::new();
        let mut cursor = name_rva;
        while cursor < image && bytes.len() < 80 {
            let unit = unsafe { *base.add(cursor) };
            if unit == 0 {
                break;
            }
            bytes.push(unit);
            cursor += 1;
        }
        Some(bytes)
    }

    unsafe fn patch_identity_import() -> bool {
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as *mut u8;
        if base.is_null() {
            return false;
        }
        let lfanew = unsafe { std::ptr::read_unaligned(base.add(0x3c).cast::<i32>()) };
        if lfanew <= 0 || lfanew > 1_048_576 {
            return false;
        }
        let nt = unsafe { base.add(lfanew as usize) };
        let magic = unsafe { std::ptr::read_unaligned(nt.add(24).cast::<u16>()) };
        if magic != 0x20b {
            return false;
        }
        let image = unsafe { std::ptr::read_unaligned(nt.add(80).cast::<u32>()) } as usize;
        if image < 256 || image > 64 * 1024 * 1024 {
            return false;
        }
        let import_rva = unsafe { std::ptr::read_unaligned(nt.add(24 + 120).cast::<u32>()) } as usize;
        if import_rva == 0 || import_rva >= image {
            return false;
        }
        let mut desc_at = import_rva;
        let mut patched_identity = false;
        for _ in 0..256 {
            if desc_at.saturating_add(20) > image {
                break;
            }
            let desc = unsafe { base.add(desc_at) };
            let original = unsafe { std::ptr::read_unaligned(desc.cast::<u32>()) };
            let name_rva = unsafe { std::ptr::read_unaligned(desc.add(12).cast::<u32>()) };
            let thunk_rva = unsafe { std::ptr::read_unaligned(desc.add(16).cast::<u32>()) };
            if original == 0 && name_rva == 0 && thunk_rva == 0 {
                break;
            }
            let mut name_rva_walk = original as usize;
            let mut slot_rva = thunk_rva as usize;
            if name_rva_walk == 0 {
                name_rva_walk = thunk_rva as usize;
            }
            for _ in 0..4096 {
                if name_rva_walk == 0 || slot_rva == 0 || name_rva_walk.saturating_add(8) > image || slot_rva.saturating_add(8) > image {
                    break;
                }
                let entry = unsafe { std::ptr::read_unaligned(base.add(name_rva_walk).cast::<u64>()) };
                if entry == 0 {
                    break;
                }
                if let Some(bytes) = unsafe { import_bytes(base, image, entry) } {
                    if let Some((store, replacement)) = hook_target(&bytes) {
                        let slot = unsafe { base.add(slot_rva).cast::<usize>() };
                        let patched = patch_slot(slot, store, replacement);
                        if bytes == b"GetFileInformationByHandle" {
                            patched_identity = patched;
                        }
                    }
                }
                name_rva_walk = name_rva_walk.saturating_add(8);
                slot_rva = slot_rva.saturating_add(8);
            }
            let Some(next) = desc_at.checked_add(20) else { break; };
            if next >= image {
                break;
            }
            desc_at = next;
        }
        patched_identity
    }

    #[cfg(any(test, rllm_module_tests))]
    pub(super) fn unclaimed_cache_stays_bounded() {
        let mut cache = IdentityCache { observed: Vec::new(), listed: Vec::new(), finds: Vec::new() };
        for index in 0..20_000u64 {
            enforce_budget(&mut cache);
            cache.observed.push(Observed {
                volume32: 0,
                index,
                attributes: 0,
                creation: 0,
                access: 0,
                write: 0,
                size: 0,
                volume64: 0,
                file_id: [0; 16],
                change: None,
                access_mode: None,
                claimed: None,
            });
            assert!(cache_len(&cache) <= CACHE_BUDGET);
        }
        assert!(cache.observed.len() < 20_000);
        assert_ne!(cache.observed.first().map(|item| item.index), Some(0));
    }

    pub(super) unsafe extern "C" fn install_file_identity() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| { unsafe { patch_identity_import(); } });
    }

    fn ensure_identity_hook() {
        unsafe { install_file_identity(); }
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
#[used]
#[link_section = ".CRT$XCU"]
static INSTALL_FILE_IDENTITY: unsafe extern "C" fn() = win::install_file_identity;

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
        if written as usize > WINDOWS_PATH_LIMIT + 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Path exceeds the extended-path limit"));
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

/// Callers pass Linux open bits. macOS uses different values for the same operations.
#[cfg_attr(
    not(any(
        test,
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )),
    allow(dead_code)
)]
fn macos_open_flags(flags: i32) -> io::Result<i32> {
    const KNOWN: i32 = 0x3 | 0x40 | 0x80 | 0x100 | 0x200 | 0x400 | 0x800 | 0x10000 | 0x20000 | 0x80000;
    if flags & !KNOWN != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported open flags"));
    }
    let mut native = flags & 0x3;
    if flags & 0x40 != 0 {
        native |= 0x200;
    }
    if flags & 0x80 != 0 {
        native |= 0x800;
    }
    if flags & 0x100 != 0 {
        native |= 0x20000;
    }
    if flags & 0x200 != 0 {
        native |= 0x400;
    }
    if flags & 0x400 != 0 {
        native |= 0x8;
    }
    if flags & 0x800 != 0 {
        native |= 0x4;
    }
    if flags & 0x10000 != 0 {
        native |= 0x100000;
    }
    if flags & 0x20000 != 0 {
        native |= 0x100;
    }
    if flags & 0x80000 != 0 {
        native |= 0x1000000;
    }
    Ok(native)
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum LinuxOpenAbi {
    Generic,
    Arm,
    Power,
}

/// Callers pass the generic open bits. ARM and Power use different values for
/// directory, no-follow, direct, and large-file. Other bits stay in place.
const fn translate_linux_open_flags(flags: i32, abi: LinuxOpenAbi) -> i32 {
    const DIRECTORY: i32 = 0x10000;
    const NOFOLLOW: i32 = 0x20000;
    const DIRECT: i32 = 0x4000;
    const LARGEFILE: i32 = 0x8000;
    let (directory, nofollow, direct, largefile) = match abi {
        LinuxOpenAbi::Generic => (DIRECTORY, NOFOLLOW, DIRECT, LARGEFILE),
        LinuxOpenAbi::Arm => (0x4000, 0x8000, 0x10000, 0x20000),
        LinuxOpenAbi::Power => (0x4000, 0x8000, 0x20000, 0x10000),
    };
    if matches!(abi, LinuxOpenAbi::Generic) {
        return flags;
    }
    let mut native = flags & !(DIRECTORY | NOFOLLOW | DIRECT | LARGEFILE);
    if flags & DIRECTORY != 0 {
        native |= directory;
    }
    if flags & NOFOLLOW != 0 {
        native |= nofollow;
    }
    if flags & DIRECT != 0 {
        native |= direct;
    }
    if flags & LARGEFILE != 0 {
        native |= largefile;
    }
    native
}

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const LINUX_OPEN_ABI: LinuxOpenAbi = LinuxOpenAbi::Arm;
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
#[cfg(any(target_arch = "powerpc", target_arch = "powerpc64"))]
const LINUX_OPEN_ABI: LinuxOpenAbi = LinuxOpenAbi::Power;
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "powerpc", target_arch = "powerpc64")))]
const LINUX_OPEN_ABI: LinuxOpenAbi = LinuxOpenAbi::Generic;

#[cfg(all(any(target_os = "linux", target_os = "android"), any(target_arch = "aarch64", target_arch = "arm")))]
const _: () = {
    let native = translate_linux_open_flags(0x10000 | 0x20000, LinuxOpenAbi::Arm);
    assert!(native == 0x4000 | 0x8000);
    assert!(native & 0x10000 == 0);
    assert!(native & 0x20000 == 0);
};

/// Callers pass the generic open bits. Each host rewrites the bits that differ before the open.
#[cfg(unix)]
pub fn native_open_flags(flags: i32) -> io::Result<i32> {
    host_open_flags(flags)
}

#[cfg(unix)]
fn host_open_flags(flags: i32) -> io::Result<i32> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return Ok(translate_linux_open_flags(flags, LINUX_OPEN_ABI));
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    {
        return macos_open_flags(flags);
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
    {
        let _ = flags;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Directory-relative open flags are not mapped for this host",
        ))
    }
}

#[cfg(unix)]
pub trait OpenOptionsExt {
    fn mode(&mut self, mode: u32) -> &mut Self;
    fn custom_flags(&mut self, flags: i32) -> &mut Self;
}

#[cfg(unix)]
impl OpenOptionsExt for OpenOptions {
    fn mode(&mut self, mode: u32) -> &mut Self {
        std::os::unix::fs::OpenOptionsExt::mode(self, mode)
    }
    fn custom_flags(&mut self, flags: i32) -> &mut Self {
        let native = native_open_flags(flags).unwrap_or(-1);
        std::os::unix::fs::OpenOptionsExt::custom_flags(self, native);
        self
    }
}

fn unit_lower(unit: u16) -> u16 {
    if (u16::from(b'A')..=u16::from(b'Z')).contains(&unit) { unit + 32 } else { unit }
}

/// A single path component that names another object: a secondary stream, a reserved
/// device, or a trailing dot or space that the host folds onto a different name.
fn wide_name_aliases(part: &[u16]) -> bool {
    if part.is_empty() || part == [u16::from(b'.')] || part == [u16::from(b'.'), u16::from(b'.')] {
        return false;
    }
    if part.iter().any(|unit| matches!(*unit, 0x22 | 0x2A | 0x3A | 0x3C | 0x3E | 0x3F | 0x7C)) {
        return true;
    }
    if matches!(part.last().copied(), Some(0x20 | 0x2E)) {
        return true;
    }
    let end = part.iter().position(|unit| *unit == u16::from(b'.')).unwrap_or(part.len());
    let base = &part[..end];
    const RESERVED: &[&[u8]] = &[
        b"con", b"prn", b"aux", b"nul", b"conin$", b"conout$",
        b"com1", b"com2", b"com3", b"com4", b"com5", b"com6", b"com7", b"com8", b"com9",
        b"lpt1", b"lpt2", b"lpt3", b"lpt4", b"lpt5", b"lpt6", b"lpt7", b"lpt8", b"lpt9",
    ];
    if RESERVED.iter().any(|name| {
        base.len() == name.len()
            && base.iter().zip(name.iter()).all(|(unit, byte)| unit_lower(*unit) == u16::from(*byte))
    }) {
        return true;
    }
    // Superscript digits ¹ ² ³ are reserved on the same device names. Later superscripts are ordinary characters.
    const SUPERSCRIPT: &[u16] = &[0x00B9, 0x00B2, 0x00B3];
    base.len() == 4
        && SUPERSCRIPT.contains(&base[3])
        && (base[..3].iter().zip(b"com").all(|(unit, byte)| unit_lower(*unit) == u16::from(*byte))
            || base[..3].iter().zip(b"lpt").all(|(unit, byte)| unit_lower(*unit) == u16::from(*byte)))
}

#[cfg_attr(not(windows), allow(dead_code))]
fn file_name_aliases(name: &str) -> bool {
    let units: Vec<u16> = name.encode_utf16().collect();
    wide_name_aliases(&units)
}

fn path_units_alias(units: &[u16]) -> bool {
    let slash = u16::from(b'\\');
    let colon = u16::from(b':');
    let mut body = units;
    let mut unc = false;
    if body.len() >= 4 && body[0] == slash && body[1] == slash && body[2] == u16::from(b'?') && body[3] == slash {
        body = &body[4..];
        let tag = [u16::from(b'U'), u16::from(b'N'), u16::from(b'C'), slash];
        if body.len() >= 4 && body[..4].iter().zip(tag).all(|(unit, expected)| unit_lower(*unit) == unit_lower(expected)) {
            unc = true;
            body = &body[4..];
        }
    } else if body.len() >= 2 && body[0] == slash && body[1] == slash {
        unc = true;
        body = &body[2..];
    }
    let mut parts = body.split(|unit| *unit == slash).filter(|part| !part.is_empty());
    if !unc {
        if let Some(first) = parts.next() {
            let drive = first.len() == 2 && first[0] <= 127 && (first[0] as u8).is_ascii_alphabetic() && first[1] == colon;
            if !drive && wide_name_aliases(first) {
                return true;
            }
        }
    }
    parts.any(wide_name_aliases)
}

fn folder_text_aliases(path: &str) -> bool {
    let units: Vec<u16> = path.encode_utf16().collect();
    path_units_alias(&units)
}

/// Open one path segment under an already opened directory.
/// `flags` uses the Linux bits the callers already pass. Each host maps those bits
/// before the open, and the open stays on the directory handle.
pub fn open_at(parent: &File, name: &[u8], flags: i32) -> io::Result<File> {
    if name.is_empty() || name.len() > WINDOWS_PATH_LIMIT {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"));
    }
    if name.contains(&0) || name.contains(&b'/') || name.contains(&b'\\') || name == b"." || name == b".." {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"));
    }
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let flags = native_open_flags(flags)?;
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
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        let text = std::str::from_utf8(name).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path segment is not Unicode"))?;
        if file_name_aliases(text) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"));
        }
        let mut units: Vec<u16> = text.encode_utf16().collect();
        if units.is_empty() || units.len() > 255 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid path segment"));
        }
        units.push(0);
        #[repr(C)]
        struct UnicodeString {
            length: u16,
            maximum_length: u16,
            buffer: *mut u16,
        }
        #[repr(C)]
        struct ObjectAttributes {
            length: u32,
            root_directory: *mut std::ffi::c_void,
            object_name: *mut UnicodeString,
            attributes: u32,
            security_descriptor: *mut std::ffi::c_void,
            security_quality_of_service: *mut std::ffi::c_void,
        }
        #[repr(C)]
        struct IoStatusBlock {
            status: usize,
            information: usize,
        }
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtCreateFile(
                handle: *mut *mut std::ffi::c_void,
                access: u32,
                object: *const ObjectAttributes,
                io: *mut IoStatusBlock,
                allocation: *const i64,
                file_attributes: u32,
                share: u32,
                disposition: u32,
                options: u32,
                ea: *mut std::ffi::c_void,
                ea_length: u32,
            ) -> i32;
            fn RtlNtStatusToDosError(status: i32) -> u32;
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CloseHandle(handle: isize) -> i32;
        }
        let mut unicode = UnicodeString {
            length: ((units.len() - 1) * 2) as u16,
            maximum_length: (units.len() * 2) as u16,
            buffer: units.as_mut_ptr(),
        };
        let object = ObjectAttributes {
            length: std::mem::size_of::<ObjectAttributes>() as u32,
            root_directory: parent.as_raw_handle(),
            object_name: &mut unicode,
            attributes: 0x40,
            security_descriptor: std::ptr::null_mut(),
            security_quality_of_service: std::ptr::null_mut(),
        };
        let mut access = 0x80 | 0x0002_0000 | 0x0010_0000;
        match flags & 3 {
            1 => access |= 0x0002 | 0x0004 | 0x0010 | 0x0100,
            2 => access |= 0x0001 | 0x0008 | 0x0002 | 0x0004 | 0x0010 | 0x0100,
            _ => access |= 0x0001 | 0x0008,
        }
        if flags & 0x10000 != 0 {
            access |= 0x0001 | 0x0020;
        }
        let disposition = if flags & 0x80 != 0 {
            2
        } else if flags & 0x40 != 0 {
            3
        } else {
            1
        };
        let mut options = 0x0000_0020 | 0x0000_4000;
        if flags & 0x20000 != 0 {
            options |= 0x0020_0000;
        }
        if flags & 0x10000 != 0 {
            options |= 0x0000_0001;
        }
        let mut io_status = IoStatusBlock { status: 0, information: 0 };
        let mut handle = std::ptr::null_mut();
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                access,
                &object,
                &mut io_status,
                std::ptr::null(),
                0x80,
                0x1 | 0x2 | 0x4,
                disposition,
                options,
                std::ptr::null_mut(),
                0,
            )
        };
        if status < 0 || handle.is_null() {
            if !handle.is_null() {
                unsafe { CloseHandle(handle as isize); }
            }
            let code = unsafe { RtlNtStatusToDosError(status) };
            return if code == 0 {
                Err(io::Error::other("directory-relative open failed"))
            } else {
                Err(io::Error::from_raw_os_error(code as i32))
            };
        }
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

/// The Android all-files permission applies to Android storage providers.
pub const ANDROID_STORAGE_PERMISSION: &str = "Enable broad file access in Workspace → Setup → Device folders and scope, or select limited folder browsing";

fn drive_path(path: &str) -> bool {
    matches!(path.as_bytes(), [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic())
}

fn unc_share(path: &str) -> bool {
    let mut parts = path.split('\\');
    [parts.next(), parts.next()].into_iter().all(|part| {
        matches!(part, Some(name) if !name.is_empty() && name != "." && name != ".."
            && !name.contains([':', '*', '?', '"', '<', '>', '|']))
    })
}

fn windows_path_text_ok(path: &str) -> bool {
    let mut units = 0usize;
    for ch in path.chars() {
        if ch.is_control() {
            return false;
        }
        units += ch.len_utf16();
        if units > WINDOWS_PATH_LIMIT {
            return false;
        }
    }
    true
}

/// Windows accepts absolute drive and share paths, including their extended forms.
pub fn windows_folder_path(path: &str) -> bool {
    if !windows_path_text_ok(path) {
        return false;
    }
    let path = path.replace('/', "\\");
    let structural = if let Some(extended) = path.strip_prefix(r"\\?\") {
        drive_path(extended)
            || (extended.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\"))
                && unc_share(&extended[4..]))
    } else {
        drive_path(&path) || path.strip_prefix(r"\\").is_some_and(unc_share)
    };
    structural && !folder_text_aliases(&path)
}

pub fn folder_permission(windows: bool, path: &str, android_granted: bool) -> Result<(), &'static str> {
    if windows {
        return if windows_folder_path(path) { Ok(()) } else { Err("Choose a folder") };
    }
    if cfg!(target_os = "android") {
        return if android_granted || path.starts_with("content://") {
            Ok(())
        } else {
            Err(ANDROID_STORAGE_PERMISSION)
        };
    }
    if path.starts_with('/') && !path.chars().any(char::is_control) {
        Ok(())
    } else {
        Err("Choose a folder")
    }
}

/// Open desktop files using the native path and the access of the current process.
pub fn desktop_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(windows)]
    {
        let _ = windows_access::backup_privileges();
        std::os::windows::fs::OpenOptionsExt::custom_flags(&mut options, 0x0200_0000);
    }
    #[cfg(unix)]
    {
        // A FIFO swapped in between a stat and an open must not hang the host.
        // Regular files ignore O_NONBLOCK.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let nonblock = 0x800;
        #[cfg(target_os = "macos")]
        let nonblock = 0x0004;
        #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
        OpenOptionsExt::custom_flags(&mut options, nonblock);
    }
    options
}

#[cfg(windows)]
mod windows_files {
    use super::*;
    use std::ffi::{c_void, OsString};
    use std::os::windows::{ffi::{OsStrExt, OsStringExt}, fs::OpenOptionsExt as WinOpen, io::AsRawHandle};
    use std::path::PathBuf;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandleEx(handle: *mut c_void, class: i32, info: *mut c_void, size: u32) -> i32;
        fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
    }

    pub fn metadata(path: &Path, follow: bool) -> io::Result<std::fs::Metadata> {
        let flags = 0x0200_0000 | if follow { 0 } else { 0x0020_0000 };
        let mut options = desktop_options();
        WinOpen::custom_flags(&mut options, flags);
        options.access_mode(0x80).open(path)?.metadata()
    }

    // An open directory handle carries the backup access into enumeration.
    pub struct Directory {
        file: File,
        path: PathBuf,
        buffer: Box<[u64; 8192]>,
        offset: Option<usize>,
        restart: bool,
        done: bool,
    }

    impl Directory {
        pub fn open(path: &Path) -> io::Result<Self> {
            let file = desktop_options().access_mode(0x0001).open(path)?;
            if !file.metadata()?.is_dir() {
                return Err(io::Error::new(io::ErrorKind::NotADirectory, "Select a directory"));
            }
            Ok(Self { file, path: path.into(), buffer: Box::new([0; 8192]), offset: None, restart: true, done: false })
        }

        fn next_entry(&mut self) -> io::Result<Option<DesktopEntry>> {
            loop {
                if self.offset.is_none() {
                    let class = if self.restart { 11 } else { 10 };
                    let ok = unsafe {
                        GetFileInformationByHandleEx(self.file.as_raw_handle(), class,
                            self.buffer.as_mut_ptr().cast(), 65536)
                    };
                    self.restart = false;
                    if ok == 0 {
                        let error = io::Error::last_os_error();
                        return if error.raw_os_error() == Some(18) { Ok(None) } else { Err(error) };
                    }
                }

                // FILE_ID_BOTH_DIR_INFO uses fixed-width fields, including the
                // filename byte length, and an inline UTF-16 name.
                let buffer = unsafe { std::slice::from_raw_parts(self.buffer.as_ptr().cast::<u8>(), 65536) };
                let start = self.offset.unwrap_or(0);
                let invalid = || io::Error::new(io::ErrorKind::InvalidData, "Invalid directory record");
                let record = buffer.get(start..).filter(|bytes| bytes.len() >= 104).ok_or_else(invalid)?;
                let field = |offset| u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap());
                let next = field(0) as usize;
                let attributes = field(56);
                let length = field(60) as usize;
                let end = 104usize.checked_add(length).ok_or_else(invalid)?;
                if length == 0 || length % 2 != 0 || end > record.len() || (next != 0 && (next < end || next > record.len())) {
                    return Err(invalid());
                }
                self.offset = if next == 0 { None } else { Some(start + next) };
                let name: Vec<u16> = record[104..end].chunks_exact(2)
                    .map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
                let name = OsString::from_wide(&name);
                if name == "." || name == ".." {
                    continue;
                }
                let directory = attributes & 0x10 != 0;
                let link = attributes & 0x400 != 0;
                return Ok(Some(DesktopEntry {
                    path: self.path.join(name),
                    directory: directory && !link,
                    file: !directory && !link,
                }));
            }
        }
    }

    impl Iterator for Directory {
        type Item = io::Result<DesktopEntry>;
        fn next(&mut self) -> Option<Self::Item> {
            if self.done {
                return None;
            }
            match self.next_entry() {
                Ok(Some(entry)) => Some(Ok(entry)),
                result => {
                    self.done = true;
                    result.transpose()
                }
            }
        }
    }

    fn ascii_eq(left: &[u16], right: &[u16]) -> bool {
        left.len() == right.len() && left.iter().zip(right).all(|(left, right)| {
            let fold = |unit: u16| if (b'A' as u16..=b'Z' as u16).contains(&unit) { unit + 32 } else { unit };
            fold(*left) == fold(*right)
        })
    }

    fn is_letter(unit: u16) -> bool {
        unit <= 127 && (unit as u8).is_ascii_alphabetic()
    }

    /// MAX_PATH is 260 UTF-16 units including the terminating NUL, so a path of
    /// 260 units or more is opened with the extended prefix. That does not depend
    /// on a process-wide long-path setting. Shorter ordinary paths are unchanged.
    pub fn widen(path: &Path) -> io::Result<PathBuf> {
        let mut units = Vec::new();
        for unit in path.as_os_str().encode_wide() {
            if units.len() >= WINDOWS_PATH_LIMIT {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path exceeds the extended-path limit"));
            }
            if unit == 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path contains NUL"));
            }
            units.push(if unit == u16::from(b'/') { u16::from(b'\\') } else { unit });
        }
        if super::path_units_alias(&units) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path names another object"));
        }
        let slash = u16::from(b'\\');
        let extended = units.len() >= 4 && units[0] == slash && units[1] == slash && units[2] == u16::from(b'?') && units[3] == slash;
        if units.len() < 260 && !extended {
            return Ok(path.to_path_buf());
        }
        verbatim(&units)
    }

    fn verbatim(units: &[u16]) -> io::Result<PathBuf> {
        let slash = u16::from(b'\\');
        let colon = u16::from(b':');
        let mut body = units;
        let mut unc = false;
        if body.len() >= 4 && body[0] == slash && body[1] == slash && body[2] == u16::from(b'?') && body[3] == slash {
            body = &body[4..];
            let tag = [u16::from(b'U'), u16::from(b'N'), u16::from(b'C'), slash];
            if body.len() >= 4 && ascii_eq(&body[..4], &tag) {
                unc = true;
                body = &body[4..];
            }
        } else if body.len() >= 2 && body[0] == slash && body[1] == slash {
            unc = true;
            body = &body[2..];
        }
        let mut out = Vec::<u16>::new();
        if unc {
            out.extend(r"\\?\UNC\".encode_utf16());
        } else {
            if body.len() < 2 || !is_letter(body[0]) || body[1] != colon || (body.len() > 2 && body[2] != slash) {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required"));
            }
            out.extend(r"\\?\".encode_utf16());
            out.push(body[0]);
            out.push(colon);
            out.push(slash);
            body = if body.len() > 2 { &body[3..] } else { &[] };
        }
        let mut stack: Vec<&[u16]> = Vec::new();
        for part in body.split(|unit| *unit == slash) {
            if part.is_empty() || part == [u16::from(b'.')] {
                continue;
            }
            if part == [u16::from(b'.'), u16::from(b'.')] {
                if stack.pop().is_none() {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path leaves its root"));
                }
                continue;
            }
            stack.push(part);
        }
        if unc && stack.len() < 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required"));
        }
        for (index, part) in stack.iter().enumerate() {
            if index > 0 {
                out.push(slash);
            }
            out.extend_from_slice(part);
        }
        if out.len() > WINDOWS_PATH_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "Path exceeds the extended-path limit"));
        }
        Ok(PathBuf::from(OsString::from_wide(&out)))
    }

    fn directory_root(wide: &[u16]) -> io::Result<usize> {
        let slash = u16::from(b'\\');
        if wide.len() >= 8 && ascii_eq(&wide[4..8], &[u16::from(b'U'), u16::from(b'N'), u16::from(b'C'), slash]) {
            let mut seps = 0;
            for (offset, unit) in wide.iter().enumerate().skip(8) {
                if *unit == slash {
                    seps += 1;
                    if seps == 2 {
                        return Ok(offset + 1);
                    }
                }
            }
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required"));
        }
        if wide.len() >= 7 && wide[5] == u16::from(b':') && wide[6] == slash {
            return Ok(7);
        }
        Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required"))
    }

    fn ensure_dir(wide: &[u16]) -> io::Result<()> {
        let slash = u16::from(b'\\');
        let root = directory_root(wide)?;
        let mut end = wide.len();
        if end > root && wide[end - 1] == slash {
            end -= 1;
        }
        if end <= root {
            return Ok(());
        }
        let path = PathBuf::from(OsString::from_wide(&wide[..end]));
        match std::fs::create_dir(&path) {
            Ok(()) => Ok(()),
            Err(error) => match super::desktop_metadata(&path, false) {
                Ok(meta) if meta.is_dir() => Ok(()),
                _ => Err(error),
            },
        }
    }

    pub fn create_directory(path: &Path, parents: bool) -> io::Result<()> {
        let path = widen(path)?;
        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let slash = u16::from(b'\\');
        let extended = wide.len() >= 4 && wide[0] == slash && wide[1] == slash && wide[2] == u16::from(b'?') && wide[3] == slash;
        if !parents {
            return std::fs::create_dir(&path);
        }
        if !extended {
            return std::fs::create_dir_all(&path);
        }
        let root = directory_root(&wide)?;
        let mut index = root;
        while index < wide.len() {
            if wide[index] == slash {
                ensure_dir(&wide[..index])?;
            }
            index += 1;
        }
        ensure_dir(&wide)
    }

    fn extended(path: &Path) -> io::Result<Vec<u16>> {
        use std::path::{Component, Prefix};
        let path = super::widen_desktop_path(path)?;
        let prefix_units: Vec<u16> = path.as_os_str().encode_wide().take(4).collect();
        if prefix_units == r"\\?\".encode_utf16().collect::<Vec<_>>() {
            let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
            wide.push(0);
            return Ok(wide);
        }
        // Absolute keeps the final component, so a link is moved rather than its target.
        let absolute = std::path::absolute(&path)?;
        let prefix = match absolute.components().next() {
            Some(Component::Prefix(prefix)) => prefix.kind(),
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required")),
        };
        let mut wide: Vec<u16> = absolute.as_os_str().encode_wide().collect();
        for unit in &mut wide {
            if *unit == u16::from(b'/') { *unit = u16::from(b'\\'); }
        }
        let mut named = match prefix {
            Prefix::Disk(_) => r"\\?\".encode_utf16().chain(wide).collect::<Vec<_>>(),
            Prefix::UNC(_, _) => r"\\?\UNC\".encode_utf16().chain(wide.into_iter().skip(2)).collect(),
            Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _) => wide,
            _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "An absolute drive or share path is required")),
        };
        named.push(0);
        Ok(named)
    }

    pub fn move_entry(source: &Path, destination: &Path) -> io::Result<()> {
        let source = extended(source)?;
        let destination = extended(destination)?;
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

/// Prepare a desktop path. On Windows, a path that does not fit in MAX_PATH
/// gains the extended prefix; an existing extended prefix is preserved.
pub fn widen_desktop_path(path: &Path) -> io::Result<std::path::PathBuf> {
    #[cfg(windows)]
    return windows_files::widen(path);
    #[cfg(not(windows))]
    Ok(path.to_path_buf())
}

/// Create one directory. `parents` creates missing ancestors and accepts an
/// existing directory. A single create still fails when the name exists.
pub fn create_directory(path: &Path, parents: bool) -> io::Result<()> {
    #[cfg(windows)]
    { return windows_files::create_directory(path, parents); }
    #[cfg(not(windows))]
    if parents { std::fs::create_dir_all(path) } else { std::fs::create_dir(path) }
}

pub fn desktop_metadata(path: &Path, follow: bool) -> io::Result<std::fs::Metadata> {
    let path = widen_desktop_path(path)?;
    #[cfg(windows)]
    return windows_files::metadata(&path, follow);
    #[cfg(not(windows))]
    if follow { std::fs::metadata(&path) } else { std::fs::symlink_metadata(&path) }
}

pub struct DesktopEntry {
    pub path: std::path::PathBuf,
    pub directory: bool,
    pub file: bool,
}

pub fn desktop_directory(path: &Path) -> io::Result<Box<dyn Iterator<Item = io::Result<DesktopEntry>>>> {
    let path = widen_desktop_path(path)?;
    #[cfg(windows)]
    {
        // Ordinary enumeration is supported by more network providers.
        // When the ACL refuses it, use the backup-capable directory handle.
        match std::fs::read_dir(&path) {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                return Ok(Box::new(windows_files::Directory::open(&path)?));
            }
            result => return result.map(directory_entries),
        }
    }
    #[cfg(not(windows))]
    std::fs::read_dir(&path).map(directory_entries)
}

fn directory_entries(entries: std::fs::ReadDir) -> Box<dyn Iterator<Item = io::Result<DesktopEntry>>> {
    Box::new(entries.map(|entry| {
        let entry = entry?;
        let kind = entry.file_type()?;
        Ok(DesktopEntry { path: entry.path(), directory: kind.is_dir(), file: kind.is_file() })
    }))
}

/// Move an entry on its own filesystem, without replacing another entry.
pub fn desktop_move(source: &Path, destination: &Path) -> io::Result<()> {
    let source = widen_desktop_path(source)?;
    let destination = widen_desktop_path(destination)?;
    #[cfg(windows)]
    return windows_files::move_entry(&source, &destination);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::{c_char, CString};
        use std::os::unix::ffi::OsStrExt;
        let path = |value: &Path| CString::new(value.as_os_str().as_bytes()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "Path contains NUL")
        });
        let source = path(&source)?;
        let destination = path(&destination)?;
        #[cfg(target_os = "linux")]
        let status = {
            unsafe extern "C" {
                fn renameat2(oldfd: i32, old: *const c_char, newfd: i32, new: *const c_char, flags: u32) -> i32;
            }
            unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) }
        };
        #[cfg(target_os = "macos")]
        let status = {
            unsafe extern "C" {
                fn renamex_np(old: *const c_char, new: *const c_char, flags: u32) -> i32;
            }
            unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), 4) }
        };
        if status == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (source, destination);
        Err(io::Error::new(io::ErrorKind::Unsupported, "A native no-replace move is unavailable on this host"))
    }
}

#[cfg(windows)]
mod windows_access {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::ptr::{null, null_mut};

    type Handle = *mut c_void;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
    const TOKEN_ELEVATION: i32 = 20;
    const TOKEN_INTEGRITY_LEVEL: i32 = 25;

    #[repr(C)]
    struct Luid {
        low: u32,
        high: i32,
    }

    #[repr(C)]
    struct Privilege {
        count: u32,
        luid: Luid,
        attributes: u32,
    }

    #[repr(C)]
    struct IntegrityLabel {
        sid: *mut c_void,
        attributes: u32,
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(token: Handle, class: i32, info: *mut c_void, len: u32, returned: *mut u32) -> i32;
        fn GetSidSubAuthority(sid: *mut c_void, index: u32) -> *mut u32;
        fn GetSidSubAuthorityCount(sid: *mut c_void) -> *mut u8;
        fn LookupPrivilegeValueW(system: *const u16, name: *const u16, luid: *mut Luid) -> i32;
        fn AdjustTokenPrivileges(token: Handle, disable_all: i32, state: *const Privilege, size: u32, previous: *mut c_void, returned: *mut u32) -> i32;
        fn DuplicateToken(token: Handle, level: i32, out: *mut Handle) -> i32;
        fn AccessCheck(
            descriptor: *const c_void,
            token: Handle,
            desired: u32,
            mapping: *const GenericMapping,
            privilege: *mut u8,
            privilege_len: *mut u32,
            granted: *mut u32,
            status: *mut i32,
        ) -> i32;
    }

    #[repr(C)]
    struct GenericMapping {
        generic_read: u32,
        generic_write: u32,
        generic_execute: u32,
        generic_all: u32,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
        fn GetKernelObjectSecurity(handle: Handle, info: u32, descriptor: *mut c_void, len: u32, needed: *mut u32) -> i32;
        fn CreateFileW(name: *const u16, access: u32, share: u32, sec: *mut c_void, disp: u32, flags: u32, template: Handle) -> Handle;
        fn GetFinalPathNameByHandleW(handle: Handle, path: *mut u16, cap: u32, flags: u32) -> u32;
    }

    fn handle_open(handle: Handle) -> bool {
        !handle.is_null() && handle != (-1isize as Handle)
    }

    fn security_descriptor(handle: Handle) -> Option<Vec<u8>> {
        if !handle_open(handle) {
            return None;
        }
        let mut needed = 0u32;
        unsafe { GetKernelObjectSecurity(handle, 7, null_mut(), 0, &mut needed); }
        if needed == 0 || needed > 65_536 {
            return None;
        }
        let mut descriptor = vec![0u8; needed as usize];
        if unsafe { GetKernelObjectSecurity(handle, 7, descriptor.as_mut_ptr().cast(), needed, &mut needed) } == 0 {
            None
        } else {
            Some(descriptor)
        }
    }

    /// The metadata handle often carries attributes only. Reading the descriptor
    /// needs the control right, so reopen the same object when the handle lacks it.
    fn descriptor_for(handle: Handle) -> Option<Vec<u8>> {
        if let Some(descriptor) = security_descriptor(handle) {
            return Some(descriptor);
        }
        let mut path = vec![0u16; 512];
        let mut length = unsafe { GetFinalPathNameByHandleW(handle, path.as_mut_ptr(), path.len() as u32, 0) };
        if length == 0 {
            return None;
        }
        if length as usize >= path.len() {
            path.resize(length as usize + 1, 0);
            length = unsafe { GetFinalPathNameByHandleW(handle, path.as_mut_ptr(), path.len() as u32, 0) };
            if length == 0 || length as usize >= path.len() {
                return None;
            }
        }
        let opened = unsafe {
            CreateFileW(path.as_ptr(), 0x0002_0000, 0x1 | 0x2 | 0x4, null_mut(), 3, 0x0200_0000 | 0x0020_0000, null_mut())
        };
        if !handle_open(opened) {
            return None;
        }
        let descriptor = security_descriptor(opened);
        unsafe { CloseHandle(opened as isize); }
        descriptor
    }

    /// One right. The call result only says the check ran. Status says whether
    /// that right was granted, and the granted mask is the bits to keep.
    fn granted_right(descriptor: *const c_void, token: Handle, mapping: *const GenericMapping, desired: u32) -> Option<u32> {
        let mut privilege = [0u8; 256];
        let mut privilege_len = privilege.len() as u32;
        let mut granted = 0u32;
        let mut status = 0i32;
        let ok = unsafe {
            AccessCheck(
                descriptor,
                token,
                desired,
                mapping,
                privilege.as_mut_ptr(),
                &mut privilege_len,
                &mut granted,
                &mut status,
            )
        };
        if ok == 0 {
            None
        } else if status == 0 {
            Some(0)
        } else {
            Some(granted)
        }
    }

    /// Effective read, write, and execute for the current process, in the owner triad.
    pub(super) fn file_access_mode(handle: Handle) -> Option<u32> {
        let descriptor = descriptor_for(handle)?;
        let mut primary = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | 0x0002, &mut primary) } == 0 {
            return None;
        }
        let mut impersonation = null_mut();
        let duplicated = unsafe { DuplicateToken(primary, 2, &mut impersonation) };
        unsafe { CloseHandle(primary as isize); }
        if duplicated == 0 {
            return None;
        }
        let mapping = GenericMapping {
            generic_read: 0x0001_2089,
            generic_write: 0x0001_2116,
            generic_execute: 0x0001_20A0,
            generic_all: 0x001F_01FF,
        };
        // Read data, write data, append, and execute. Write is either data or append.
        let mut mask = 0u32;
        let mut answered = false;
        for desired in [0x1u32, 0x2, 0x4, 0x20] {
            if let Some(bits) = granted_right(descriptor.as_ptr().cast(), impersonation, &mapping, desired) {
                answered = true;
                mask |= bits;
            }
        }
        unsafe { CloseHandle(impersonation as isize); }
        if answered { Some(mask) } else { None }
    }

    struct Token(Handle);

    impl Token {
        fn open(access: u32) -> io::Result<Self> {
            let mut token = null_mut();
            let ok = unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) };
            if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(Self(token)) }
        }

        fn elevated(&self) -> io::Result<bool> {
            let mut elevated = 0u32;
            let mut returned = 0;
            let ok = unsafe {
                GetTokenInformation(self.0, TOKEN_ELEVATION, (&mut elevated as *mut u32).cast(), 4, &mut returned)
            };
            if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(elevated != 0) }
        }

        fn integrity(&self) -> io::Result<u32> {
            // The SID storage returned by the token query shares the label buffer.
            let mut raw = [0usize; 64];
            let mut returned = 0;
            unsafe {
                if GetTokenInformation(self.0, TOKEN_INTEGRITY_LEVEL, raw.as_mut_ptr().cast(),
                    size_of::<[usize; 64]>() as u32, &mut returned) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let sid = (*raw.as_ptr().cast::<IntegrityLabel>()).sid;
                if !sid.is_null() {
                    let count = *GetSidSubAuthorityCount(sid);
                    if count != 0 {
                        return Ok(*GetSidSubAuthority(sid, u32::from(count - 1)));
                    }
                }
            }
            Err(io::Error::new(io::ErrorKind::InvalidData, "Process token has no integrity SID"))
        }

        fn enable(&self, name: &str) -> io::Result<()> {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            let mut state = Privilege { count: 1, luid: Luid { low: 0, high: 0 }, attributes: 2 };
            let code = unsafe {
                if LookupPrivilegeValueW(null(), name.as_ptr(), &mut state.luid) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let ok = AdjustTokenPrivileges(self.0, 0, &state, 0, null_mut(), null_mut());
                let code = GetLastError();
                if ok == 0 && code == 0 {
                    return Err(io::Error::other("The process token adjustment failed"));
                }
                code
            };
            if code == 0 { Ok(()) } else { Err(io::Error::from_raw_os_error(code as i32)) }
        }
    }

    impl Drop for Token {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0 as isize); }
        }
    }

    pub fn elevated() -> io::Result<bool> {
        Token::open(TOKEN_QUERY)?.elevated()
    }

    pub fn backup_privileges() -> Result<&'static str, String> {
        static PRIVILEGES: std::sync::OnceLock<Result<&'static str, String>> = std::sync::OnceLock::new();
        PRIVILEGES.get_or_init(|| {
            if !elevated().map_err(|error| error.to_string())? {
                return Err("Backup and restore privileges require an elevated process. Use Restart elevated to open the Windows consent prompt.".into());
            }
            let token = Token::open(TOKEN_QUERY | TOKEN_ADJUST_PRIVILEGES).map_err(|error| error.to_string())?;
            let backup = token.enable("SeBackupPrivilege");
            let restore = token.enable("SeRestorePrivilege");
            match (backup, restore) {
                (Ok(()), Ok(())) => Ok("Backup and restore privileges enabled."),
                (backup, restore) => {
                    let status = |result: io::Result<()>| match result {
                        Ok(()) => "enabled".into(),
                        Err(error) => error.to_string(),
                    };
                    Err(format!("Backup privilege: {}. Restore privilege: {}.", status(backup), status(restore)))
                }
            }
        }).clone()
    }

    pub fn process_access() -> String {
        let token = match Token::open(TOKEN_QUERY) {
            Ok(token) => token,
            Err(error) => return format!("Process token unavailable: {error}"),
        };
        let integrity = match token.integrity() {
            Ok(rid) => {
                let label = match rid {
                    0x0000..=0x0fff => "Untrusted",
                    0x1000..=0x1fff => "Low",
                    0x2000..=0x20ff => "Medium",
                    0x2100..=0x2fff => "Medium plus",
                    0x3000..=0x3fff => "High",
                    0x4000..=0x4fff => "System",
                    0x5000 => "Protected process",
                    _ => "Unrecognized",
                };
                format!("{label} integrity (0x{rid:04x})")
            }
            Err(error) => format!("Integrity unavailable ({error})"),
        };
        match token.elevated() {
            Ok(true) => {
                let privileges = match backup_privileges() {
                    Ok(value) => value.to_string(),
                    Err(error) => error,
                };
                format!("{integrity}. Elevated process. {privileges}")
            }
            Ok(false) => format!("{integrity}. Standard user (not elevated). Restart elevated opens the Windows consent prompt."),
            Err(error) => format!("{integrity}. Token elevation unavailable ({error})."),
        }
    }
}

#[cfg(windows)]
pub use windows_access::{backup_privileges, elevated};

#[cfg(unix)]
pub fn effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}

/// Describe current process credentials as reported by the operating system.
pub fn process_access() -> String {
    #[cfg(windows)]
    return windows_access::process_access();
    #[cfg(unix)]
    {
        let uid = effective_uid();
        let mut access = format!("Effective UID {uid}.");
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let capabilities = std::fs::read_to_string("/proc/thread-self/status").ok().and_then(|status| {
                status.lines().find_map(|line| line.strip_prefix("CapEff:")).and_then(|hex| {
                    u64::from_str_radix(hex.trim(), 16).ok()
                })
            });
            match capabilities {
                Some(mask) => access.push_str(&format!(" Effective capabilities 0x{mask:016x}.")),
                None => access.push_str(" Effective capabilities unavailable."),
            }
        }
        if uid != 0 {
            if cfg!(target_os = "android") {
                access.push_str(" Broad file access is a separate device setting.");
            } else {
                access.push_str(" For elevated access, start RLLM as root.");
            }
        }
        return access;
    }
    #[cfg(not(any(windows, unix)))]
    "Process credentials are unavailable on this host.".into()
}

#[cfg(any(test, rllm_module_tests))]
mod checks {
    #[cfg_attr(not(rllm_module_tests), test)]
    fn folder_permission_uses_the_native_path_format() {
        use super::folder_permission;
        for path in [r"C:\", "D:/work/tree", r"\\server\share", r"\\?\C:\long", r"\\?\UNC\server\share\long"] {
            assert_eq!(folder_permission(true, path, false), Ok(()), "{path}");
        }
        for path in [r"C:relative", r"\\server", r"\\?\GLOBALROOT\Device", r"\\.\C:", "content://tree", "C:\\bad\npath",
            r"C:\foo:bar", r"C:\CON", r"C:\dir\nul.txt", r"C:\dir\file.", r"C:\dir\file ", r"D:\com1\a",
            r"\\server\share\foo.", r"\\?\C:\foo\bar:stream", r"\\?\UNC\server\share\aux.txt"] {
            assert!(folder_permission(true, path, false).is_err(), "{path}");
        }
        #[cfg(not(target_os = "android"))]
        assert_eq!(folder_permission(false, "/var/tmp", false), Ok(()));
        #[cfg(target_os = "android")]
        assert_eq!(folder_permission(false, "/sdcard", false), Err(super::ANDROID_STORAGE_PERMISSION));
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn macos_open_translates_linux_nofollow_and_create_flags() {
        use super::macos_open_flags;
        let nofollow = macos_open_flags(0x20000).unwrap();
        assert_eq!(nofollow, 0x100);
        assert_eq!(nofollow & 0x20000, 0);
        let create = macos_open_flags(1 | 0x40 | 0x80 | 0x20000).unwrap();
        assert_eq!(create, 1 | 0x200 | 0x800 | 0x100);
        assert_ne!(create, 1 | 0x40 | 0x80 | 0x20000);
        let directory = macos_open_flags(0x80000 | 0x20000 | 0x800 | 0x10000).unwrap();
        assert_eq!(directory, 0x1000000 | 0x100 | 0x4 | 0x100000);
        assert_eq!(macos_open_flags(0x100).unwrap(), 0x20000);
        assert!(macos_open_flags(0x200000).is_err());
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn oversized_windows_path_is_rejected_before_use() {
        use super::{widen_desktop_path, windows_folder_path, WINDOWS_PATH_LIMIT};
        let mut at_limit = String::from(r"C:\");
        at_limit.push_str(&"a".repeat(WINDOWS_PATH_LIMIT - 3));
        assert_eq!(at_limit.chars().count(), WINDOWS_PATH_LIMIT);
        assert!(windows_folder_path(&at_limit));
        let mut over = at_limit.clone();
        over.push('b');
        assert!(!windows_folder_path(&over));
        let mut hostile = String::from(r"C:\");
        hostile.extend(std::iter::repeat('a').take(40_000));
        assert!(!windows_folder_path(&hostile));
        assert!(hostile.len() > WINDOWS_PATH_LIMIT);
        #[cfg(windows)]
        {
            let widened = widen_desktop_path(std::path::Path::new(&hostile)).unwrap_err();
            assert_eq!(widened.kind(), std::io::ErrorKind::InvalidInput);
            let mut long = String::from(r"C:\");
            long.push_str(&"a".repeat(300));
            let text = widen_desktop_path(std::path::Path::new(&long)).unwrap();
            let shown = text.to_string_lossy();
            assert!(shown.starts_with(r"\\?\C:\"), "{shown}");
            let alias = widen_desktop_path(std::path::Path::new(r"C:\folder\file.")).unwrap_err();
            assert_eq!(alias.kind(), std::io::ErrorKind::InvalidInput);
            let stream = widen_desktop_path(std::path::Path::new(r"C:\folder\base.txt:hidden")).unwrap_err();
            assert_eq!(stream.kind(), std::io::ErrorKind::InvalidInput);
        }
    }

    #[cfg(windows)]
    fn scratch(name: &str) -> std::path::PathBuf {
        let mut path = std::env::current_exe().unwrap();
        path.pop();
        path.push("tmp");
        path.push(format!("{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(windows)]
    fn open_dir(path: &std::path::Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).custom_flags(0x0200_0000).open(path).unwrap()
    }

    #[cfg(windows)]
    fn os_identity(file: &std::fs::File) -> (u64, u64) {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct Info {
            attributes: u32,
            c_lo: u32,
            c_hi: u32,
            a_lo: u32,
            a_hi: u32,
            w_lo: u32,
            w_hi: u32,
            volume: u32,
            size_hi: u32,
            size_lo: u32,
            links: u32,
            index_hi: u32,
            index_lo: u32,
        }
        unsafe extern "system" {
            fn GetFileInformationByHandle(handle: *mut std::ffi::c_void, info: *mut Info) -> i32;
        }
        let mut info = std::mem::MaybeUninit::<Info>::zeroed();
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) };
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        let info = unsafe { info.assume_init() };
        (u64::from(info.volume), ((info.index_hi as u64) << 32) | u64::from(info.index_lo))
    }

    #[cfg(windows)]
    fn published_identity(file: &std::fs::File) -> (u64, u64, [u8; 16]) {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct FileIdInfo { volume: u64, id: [u8; 16] }
        unsafe extern "system" {
            fn GetFileInformationByHandleEx(handle: *mut std::ffi::c_void, class: i32, info: *mut std::ffi::c_void, size: u32) -> i32;
        }
        let mut info = std::mem::MaybeUninit::<FileIdInfo>::zeroed();
        let ok = unsafe {
            GetFileInformationByHandleEx(file.as_raw_handle(), 18, info.as_mut_ptr().cast(), std::mem::size_of::<FileIdInfo>() as u32)
        };
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        let info = unsafe { info.assume_init() };
        let published = super::win::publish_identity(info.volume, &info.id);
        (published.0, published.1, info.id)
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn windows_identity_is_volume_serial_and_file_index() {
        use super::MetadataExt;
        use std::os::windows::fs::MetadataExt as WinMeta;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("identity");
        let first_path = root.join("first.txt");
        let second_path = root.join("second.txt");
        std::fs::write(&first_path, b"one").unwrap();
        std::fs::write(&second_path, b"two!!").unwrap();
        let stamp = 0x01DC_0000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let link = root.join("first-link.txt");
        std::fs::hard_link(&first_path, &link).unwrap();
        let first = std::fs::metadata(&first_path).unwrap();
        let second = std::fs::metadata(&second_path).unwrap();
        let linked = std::fs::metadata(&link).unwrap();
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (vol_a, idx_a) = os_identity(&first_file);
        let (vol_b, idx_b) = os_identity(&second_file);
        let (dev_a, ino_a, id_a) = published_identity(&first_file);
        let (dev_b, ino_b, id_b) = published_identity(&second_file);
        assert_eq!((first.dev(), first.ino()), (dev_a, ino_a));
        assert_eq!((second.dev(), second.ino()), (dev_b, ino_b));
        assert_eq!(u64::from_le_bytes(id_a[..8].try_into().unwrap()), idx_a);
        assert_eq!(u64::from_le_bytes(id_b[..8].try_into().unwrap()), idx_b);
        assert_eq!(dev_a as u32, vol_a as u32);
        assert_eq!(dev_b as u32, vol_b as u32);
        assert_eq!(first.dev(), second.dev());
        assert_ne!(first.ino(), second.ino());
        assert_eq!(linked.dev(), first.dev());
        assert_eq!(linked.ino(), first.ino());
        assert_eq!(WinMeta::creation_time(&first), WinMeta::creation_time(&second));
        assert_eq!(WinMeta::creation_time(&first), stamp);
        assert_ne!(first.ino(), WinMeta::creation_time(&first));
        assert_ne!(second.ino(), WinMeta::creation_time(&second));
        std::fs::remove_file(&first_path).unwrap();
        std::fs::write(&first_path, b"replaced").unwrap();
        let replaced_file = std::fs::OpenOptions::new().read(true).write(true).open(&first_path).unwrap();
        assert_ne!(unsafe { SetFileTime(replaced_file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        let replaced = std::fs::metadata(&first_path).unwrap();
        assert_eq!(WinMeta::creation_time(&replaced), stamp);
        assert_eq!(replaced.dev(), first.dev());
        assert_ne!(replaced.ino(), first.ino());
        assert_ne!(replaced.ino(), idx_a);
        let (replaced_dev, replaced_ino, _) = published_identity(&replaced_file);
        assert_eq!((replaced.dev(), replaced.ino()), (replaced_dev, replaced_ino));
        let directory = std::fs::metadata(&root).unwrap();
        let directory_file = open_dir(&root);
        let (dir_dev, dir_ino, _) = published_identity(&directory_file);
        assert_eq!((directory.dev(), directory.ino()), (dir_dev, dir_ino));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn open_at_uses_the_directory_handle() {
        use super::{open_at, MetadataExt};
        use std::io::{Read, Write};
        let root = scratch("open-at");
        let orig = root.join("orig");
        let other = root.join("other");
        std::fs::create_dir(&orig).unwrap();
        std::fs::create_dir(&other).unwrap();
        std::fs::create_dir(orig.join("sub")).unwrap();
        std::fs::write(orig.join("sub").join("nested.txt"), b"nested").unwrap();
        std::fs::write(orig.join("kept.txt"), b"kept").unwrap();
        std::fs::write(other.join("kept.txt"), b"other").unwrap();
        let parent = open_dir(&orig);
        let sub = open_at(&parent, b"sub", 0x10000).unwrap();
        assert!(sub.metadata().unwrap().is_dir());
        let mut nested = open_at(&sub, b"nested.txt", 0).unwrap();
        let mut text = String::new();
        nested.read_to_string(&mut text).unwrap();
        assert_eq!(text, "nested");
        let mut created = open_at(&parent, b"new.txt", 1 | 0x40 | 0x80 | 0x20000).unwrap();
        created.write_all(b"abc").unwrap();
        drop(created);
        let again = open_at(&parent, b"new.txt", 1 | 0x40 | 0x80 | 0x20000).unwrap_err();
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        let mut read_back = open_at(&parent, b"new.txt", 0x20000).unwrap();
        let mut body = String::new();
        read_back.read_to_string(&mut body).unwrap();
        assert_eq!(body, "abc");
        for bad in [b"".as_slice(), b".", b"..", b"a/b", b"a\\b", b"a\0b"] {
            assert_eq!(open_at(&parent, bad, 0).unwrap_err().kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
        }
        let oversized = vec![b'a'; 40_000];
        assert_eq!(open_at(&parent, &oversized, 0).unwrap_err().kind(), std::io::ErrorKind::InvalidInput);
        drop(sub);
        drop(nested);
        drop(read_back);
        let parked = root.join("parked");
        std::fs::rename(&orig, &parked).unwrap();
        std::fs::rename(&other, &orig).unwrap();
        let mut kept = open_at(&parent, b"kept.txt", 0).unwrap();
        let mut got = String::new();
        kept.read_to_string(&mut got).unwrap();
        assert_eq!(got, "kept");
        let kept_meta = std::fs::metadata(parked.join("kept.txt")).unwrap();
        assert_eq!(kept.metadata().unwrap().ino(), kept_meta.ino());
        assert_eq!(kept.metadata().unwrap().dev(), kept_meta.dev());
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn open_at_ignores_a_swapped_directory_path() {
        use super::{open_at, MetadataExt};
        use std::io::Read;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let root = scratch("swap");
        let orig = root.join("orig");
        let decoy = root.join("decoy");
        let hold = root.join("hold");
        std::fs::create_dir(&orig).unwrap();
        std::fs::create_dir(&decoy).unwrap();
        std::fs::write(orig.join("secret"), b"original").unwrap();
        std::fs::write(decoy.join("secret"), b"decoy").unwrap();
        let parent = open_dir(&orig);
        let expected = std::fs::metadata(orig.join("secret")).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let orig_m = orig.clone();
        let decoy_m = decoy.clone();
        let hold_m = hold.clone();
        let mover = std::thread::spawn(move || {
            while !flag.load(Ordering::Relaxed) {
                let _ = std::fs::rename(&orig_m, &hold_m);
                let _ = std::fs::rename(&decoy_m, &orig_m);
                let _ = std::fs::rename(&hold_m, &decoy_m);
                let _ = std::fs::rename(&decoy_m, &hold_m);
                let _ = std::fs::rename(&orig_m, &decoy_m);
                let _ = std::fs::rename(&hold_m, &orig_m);
            }
        });
        for _ in 0..250 {
            let mut file = open_at(&parent, b"secret", 0).expect("handle-relative open");
            let mut buf = Vec::new();
            file.read_to_end(&mut buf).unwrap();
            assert_eq!(buf, b"original");
            let meta = file.metadata().unwrap();
            assert_eq!((meta.dev(), meta.ino()), (expected.dev(), expected.ino()));
        }
        stop.store(true, Ordering::Relaxed);
        mover.join().unwrap();
    }

    #[cfg(windows)]
    fn set_junction(link: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        let canonical = std::fs::canonicalize(target)?;
        let mut display = canonical.to_string_lossy().replace('/', "\\");
        if let Some(rest) = display.strip_prefix(r"\\?\UNC\") {
            display = format!(r"\\{rest}");
        } else if let Some(rest) = display.strip_prefix(r"\\?\") {
            display = rest.to_string();
        }
        let print = display.trim_end_matches('\\').to_string();
        let substitute = format!(r"\??\{print}");
        let sub: Vec<u16> = substitute.encode_utf16().collect();
        let printed: Vec<u16> = print.encode_utf16().collect();
        let mut path = Vec::new();
        path.extend_from_slice(&sub);
        path.push(0);
        let print_offset = path.len() * 2;
        path.extend_from_slice(&printed);
        path.push(0);
        let path_bytes = path.len() * 2;
        let data_length = 8 + path_bytes;
        let mut buf = Vec::<u8>::new();
        buf.extend_from_slice(&0xA000_0003u32.to_le_bytes());
        buf.extend_from_slice(&(data_length as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&((sub.len() * 2) as u16).to_le_bytes());
        buf.extend_from_slice(&(print_offset as u16).to_le_bytes());
        buf.extend_from_slice(&((printed.len() * 2) as u16).to_le_bytes());
        for unit in path {
            buf.extend_from_slice(&unit.to_le_bytes());
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .open(link)?;
        unsafe extern "system" {
            fn DeviceIoControl(
                handle: *mut std::ffi::c_void,
                code: u32,
                input: *const std::ffi::c_void,
                input_size: u32,
                output: *mut std::ffi::c_void,
                output_size: u32,
                returned: *mut u32,
                overlapped: *mut std::ffi::c_void,
            ) -> i32;
        }
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                0x0009_00A4,
                buf.as_ptr().cast(),
                buf.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn open_at_nofollow_does_not_follow_a_junction() {
        use super::{open_at, MetadataExt};
        use std::os::windows::fs::MetadataExt as WinMeta;
        let root = scratch("junction");
        let target = root.join("target");
        let link = root.join("link");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("marker.txt"), b"target").unwrap();
        std::fs::create_dir(&link).unwrap();
        set_junction(&link, &target).unwrap();
        let link_meta = std::fs::symlink_metadata(&link).unwrap();
        let target_meta = std::fs::metadata(&target).unwrap();
        assert_ne!(link_meta.ino(), target_meta.ino());
        assert_eq!(link_meta.dev(), target_meta.dev());
        let parent = open_dir(&root);
        let followed = open_at(&parent, b"link", 0).unwrap();
        let held = open_at(&parent, b"link", 0x20000).unwrap();
        assert_eq!(followed.metadata().unwrap().ino(), target_meta.ino());
        assert_eq!(followed.metadata().unwrap().dev(), target_meta.dev());
        assert_eq!(held.metadata().unwrap().ino(), link_meta.ino());
        assert_eq!(held.metadata().unwrap().dev(), link_meta.dev());
        assert_ne!(WinMeta::file_attributes(&held.metadata().unwrap()) & 0x400, 0);
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn pre_1970_filetime_keeps_the_negative_second() {
        let epoch = 11_644_473_600u64 * 10_000_000;
        assert_eq!(super::win::filetime(0), (-11_644_473_600, 0));
        assert_eq!(super::win::filetime(epoch), (0, 0));
        assert_eq!(super::win::filetime(epoch - 1), (-1, 999_999_900));
        assert_eq!(super::win::filetime(epoch - 5_000_000), (-1, 500_000_000));
        assert_eq!(super::win::filetime(86_400 * 10_000_000), (86_400 - 11_644_473_600, 0));
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("ancient");
        let path = root.join("ancient.txt");
        std::fs::write(&path, b"old").unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let ticks = 86_400 * 10_000_000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &ticks, &ticks, &ticks) }, 0);
        let meta = file.metadata().unwrap();
        assert_eq!(meta.mtime(), 86_400 - 11_644_473_600);
        assert_eq!(meta.mtime_nsec(), 0);
        assert!(meta.mtime() < 0);
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn high_half_of_file_id_is_part_of_identity() {
        let mut low = [0u8; 16];
        low[..8].copy_from_slice(&0x11u64.to_le_bytes());
        let mut high = low;
        high[8..].copy_from_slice(&0x22u64.to_le_bytes());
        let volume = 0xABCD_u64;
        let plain = super::win::publish_identity(volume, &low);
        let extended = super::win::publish_identity(volume, &high);
        assert_eq!(plain, (volume, 0x11));
        assert_ne!(plain, extended);
        assert_eq!(extended, super::win::publish_identity(volume, &high));
        let mut other_low = [0u8; 16];
        other_low[..8].copy_from_slice(&0x33u64.to_le_bytes());
        other_low[8..].copy_from_slice(&0x22u64.to_le_bytes());
        assert_ne!(extended, super::win::publish_identity(volume, &other_low));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn restored_write_time_does_not_hide_a_change() {
        use super::MetadataExt;
        use std::os::windows::fs::MetadataExt as WinMeta;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("ctime");
        let path = root.join("edited.txt");
        std::fs::write(&path, b"same").unwrap();
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        let stamp = 0x01DC_0000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        drop(file);
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let before = std::fs::metadata(&path).unwrap();
        std::fs::write(&path, b"SAME").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), std::ptr::null(), std::ptr::null(), &stamp) }, 0);
        drop(file);
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(before.mtime(), after.mtime());
        assert_eq!(before.len(), after.len());
        assert_eq!(WinMeta::creation_time(&before), WinMeta::creation_time(&after));
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_ne!((before.ctime(), before.ctime_nsec()), (after.ctime(), after.ctime_nsec()));
        assert_ne!((before.ctime(), before.ctime_nsec()), super::win::filetime(stamp));
        assert_ne!((after.ctime(), after.ctime_nsec()), super::win::filetime(stamp));
        assert_ne!((before.ctime(), before.ctime_nsec()), super::win::filetime(WinMeta::creation_time(&before)));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn index_less_metadata_is_not_the_creation_time() {
        use super::MetadataExt;
        use std::os::windows::fs::MetadataExt as WinMeta;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("fallback");
        let first_path = root.join("one.txt");
        let second_path = root.join("two.txt");
        std::fs::write(&first_path, b"one").unwrap();
        std::fs::write(&second_path, b"two!").unwrap();
        let stamp = 0x01DC_0000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let mut first = None;
        let mut second = None;
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let name = entry.file_name();
            let meta = entry.metadata().unwrap();
            if name == "one.txt" { first = Some(meta); }
            else if name == "two.txt" { second = Some(meta); }
        }
        let first = first.unwrap();
        let second = second.unwrap();
        let created = WinMeta::creation_time(&first);
        assert_eq!(created, WinMeta::creation_time(&second));
        assert_ne!(WinMeta::file_size(&first), WinMeta::file_size(&second));
        assert_eq!(WinMeta::last_access_time(&first), WinMeta::last_access_time(&second));
        assert_eq!(WinMeta::last_write_time(&first), WinMeta::last_write_time(&second));
        // Each directory record opens its own path and reports that file's id.
        assert_ne!((first.dev(), first.ino()), (0, 0));
        assert_ne!((second.dev(), second.ino()), (0, 0));
        assert_ne!((first.dev(), first.ino()), (second.dev(), second.ino()));
        assert_ne!(first.ino(), created);
        assert_eq!(first.ctime(), super::win::CHANGE_UNAVAILABLE);
        assert_eq!(first.ctime_nsec(), super::win::CHANGE_UNAVAILABLE_NSEC);
        assert_eq!(first.ctime_nsec(), first.ctime_nsec());
        assert_eq!(second.ctime(), super::win::CHANGE_UNAVAILABLE);
        assert_eq!(second.ctime_nsec(), super::win::CHANGE_UNAVAILABLE_NSEC);
        assert_eq!((first.ctime(), first.ctime_nsec()), (second.ctime(), second.ctime_nsec()));
        assert_ne!(first.ctime(), super::win::filetime(created).0);
        assert_eq!(first.ino(), first.ino());
        assert_eq!(first.ctime(), first.ctime());
        let indexed = std::fs::metadata(&first_path).unwrap();
        let indexed_file = std::fs::File::open(&first_path).unwrap();
        let (indexed_dev, indexed_ino, _) = published_identity(&indexed_file);
        assert_eq!((indexed.dev(), indexed.ino()), (indexed_dev, indexed_ino));
        assert_ne!((indexed.dev(), indexed.ino()), (0, 0));
        assert_eq!((indexed.dev(), indexed.ino()), (first.dev(), first.ino()));
        let indexed_pair = (indexed.ctime(), indexed.ctime_nsec());
        assert_eq!(indexed_pair, (indexed.ctime(), indexed.ctime_nsec()));
        assert_ne!(indexed_pair, (first.ctime(), first.ctime_nsec()));
        let left_file = std::fs::File::open(&first_path).unwrap();
        let right_file = std::fs::File::open(&second_path).unwrap();
        let (left_dev, left_ino, _) = published_identity(&left_file);
        let (right_dev, right_ino, _) = published_identity(&right_file);
        assert_ne!((left_dev, left_ino), (right_dev, right_ino));
        assert_ne!((left_dev, left_ino), (0, 0));
        assert_eq!((first.dev(), first.ino()), (left_dev, left_ino));
        assert_eq!((second.dev(), second.ino()), (right_dev, right_ino));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn open_at_rejects_stream_device_and_trailing_names() {
        use super::open_at;
        use std::io::{Read, Write};
        let root = scratch("alias");
        let base = root.join("base.txt");
        std::fs::write(&base, b"visible").unwrap();
        let parent = open_dir(&root);
        for bad in [
            b"base.txt:hidden".as_slice(),
            b"base.txt:$DATA",
            b":hidden",
            b"CON",
            b"con.txt",
            b"PRN",
            b"AUX",
            b"NUL",
            b"COM1",
            b"lpt9.dat",
            b"foo.",
            b"foo ",
            b"bar..",
            b"file::$DATA",
        ] {
            let err = open_at(&parent, bad, 1 | 0x40 | 0x80).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{bad:?}");
        }
        assert_eq!(std::fs::read(&base).unwrap(), b"visible");
        assert!(std::fs::metadata(root.join("base.txt:hidden")).is_err());
        assert!(!root.join("foo").exists());
        assert!(!root.join("CON").exists());
        let names: Vec<_> = std::fs::read_dir(&root).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert!(names.iter().all(|name| !name.to_string_lossy().contains(':')));
        assert_eq!(names.iter().filter(|name| *name == "base.txt").count(), 1);
        let mut created = open_at(&parent, b"plain.txt", 1 | 0x40 | 0x80).unwrap();
        created.write_all(b"ok").unwrap();
        drop(created);
        let mut read_back = open_at(&parent, b"plain.txt", 0).unwrap();
        let mut body = Vec::new();
        read_back.read_to_end(&mut body).unwrap();
        assert_eq!(body, b"ok");
    }

    #[cfg(windows)]
    fn change_ticks(file: &std::fs::File) -> u64 {
        use std::os::windows::io::AsRawHandle;
        #[repr(C)]
        struct Basic { creation: i64, access: i64, write: i64, change: i64, attributes: u32 }
        unsafe extern "system" {
            fn GetFileInformationByHandleEx(handle: *mut std::ffi::c_void, class: i32, info: *mut std::ffi::c_void, size: u32) -> i32;
        }
        let mut info = std::mem::MaybeUninit::<Basic>::zeroed();
        let ok = unsafe {
            GetFileInformationByHandleEx(file.as_raw_handle(), 0, info.as_mut_ptr().cast(), std::mem::size_of::<Basic>() as u32)
        };
        assert_ne!(ok, 0, "{}", std::io::Error::last_os_error());
        let change = unsafe { info.assume_init() }.change;
        assert!(change >= 0);
        change as u64
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn directory_metadata_keeps_distinct_os_identities_when_the_address_is_reused() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("reused-address");
        let first_path = root.join("a.txt");
        let second_path = root.join("b.txt");
        std::fs::write(&first_path, b"same").unwrap();
        std::fs::write(&second_path, b"same!").unwrap();
        let stamp = 0x01DC_1000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let mut listed = Vec::new();
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let name = entry.file_name();
            if name == "a.txt" || name == "b.txt" {
                let meta = entry.metadata().unwrap();
                let again = (meta.ctime(), meta.ctime_nsec());
                listed.push((name, meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()));
                assert_eq!(again, (meta.ctime(), meta.ctime_nsec()));
            }
        }
        assert_eq!(listed.len(), 2);
        assert_ne!((listed[0].1, listed[0].2), (0, 0));
        assert_ne!((listed[1].1, listed[1].2), (0, 0));
        assert_eq!(listed[0].3, super::win::CHANGE_UNAVAILABLE);
        assert_eq!(listed[0].4, super::win::CHANGE_UNAVAILABLE_NSEC);
        assert_eq!(listed[1].3, super::win::CHANGE_UNAVAILABLE);
        assert_eq!((listed[0].3, listed[0].4), (listed[1].3, listed[1].4));
        let mut slot = std::fs::metadata(&first_path).unwrap();
        let address_a = &slot as *const std::fs::Metadata as usize;
        let first = (slot.dev(), slot.ino());
        let first_change = (slot.ctime(), slot.ctime_nsec());
        assert_eq!(first_change, (slot.ctime(), slot.ctime_nsec()));
        slot = std::fs::metadata(&second_path).unwrap();
        let address_b = &slot as *const std::fs::Metadata as usize;
        let second = (slot.dev(), slot.ino());
        let second_change = (slot.ctime(), slot.ctime_nsec());
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (expected_a_dev, expected_a_ino, _) = published_identity(&first_file);
        let (expected_b_dev, expected_b_ino, _) = published_identity(&second_file);
        assert_eq!(address_a, address_b);
        assert_eq!(first, (expected_a_dev, expected_a_ino));
        assert_eq!(second, (expected_b_dev, expected_b_ino));
        assert_ne!(first, second);
        assert_ne!(second, (0, 0));
        for item in &listed {
            let expected = if item.0 == "a.txt" { first } else { second };
            let other = if item.0 == "a.txt" { second } else { first };
            assert_eq!((item.1, item.2), expected, "{item:?}");
            assert_ne!((item.1, item.2), other, "{item:?}");
        }
        assert_ne!(first_change.0, super::win::CHANGE_UNAVAILABLE);
        assert_ne!(second_change.0, super::win::CHANGE_UNAVAILABLE);
        assert_eq!(first_change, (super::win::filetime(change_ticks(&first_file))));
        assert_eq!(second_change, (super::win::filetime(change_ticks(&second_file))));
        assert_eq!(second, (slot.dev(), slot.ino()));
        assert_eq!(second_change, (slot.ctime(), slot.ctime_nsec()));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn unclaimed_observation_cache_cannot_grow_without_a_bound() {
        super::win::unclaimed_cache_stays_bounded();
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn arm_linux_open_flags_keep_directory_and_nofollow() {
        use super::{translate_linux_open_flags, LinuxOpenAbi};
        let asked = 0x80000 | 0x20000 | 0x800 | 0x10000;
        let arm = translate_linux_open_flags(asked, LinuxOpenAbi::Arm);
        assert_eq!(arm & 0x4000, 0x4000, "directory open bit");
        assert_eq!(arm & 0x8000, 0x8000, "no-follow bit");
        assert_eq!(arm & 0x10000, 0, "generic directory bit must not remain the direct bit");
        assert_eq!(arm & 0x20000, 0, "generic no-follow bit must not remain the large-file bit");
        assert_eq!(arm & 0x80000, 0x80000);
        assert_eq!(arm & 0x800, 0x800);
        assert_eq!(translate_linux_open_flags(asked, LinuxOpenAbi::Generic), asked);
        assert_eq!(translate_linux_open_flags(0x4000 | 0x8000, LinuxOpenAbi::Arm), 0x10000 | 0x20000);
        assert_eq!(translate_linux_open_flags(0x10000 | 0x20000, LinuxOpenAbi::Power), 0x4000 | 0x8000);
        assert_eq!(translate_linux_open_flags(0x4000, LinuxOpenAbi::Power), 0x20000);
        assert_eq!(translate_linux_open_flags(0x8000, LinuxOpenAbi::Power), 0x10000);
        // Generic directory and no-follow bits used by connected opens, including
        // the nonblocking bit paired with no-follow.
        for flags in [0x200000 | 0x20000 | 0x80000, 0x10000 | 0x20000 | 0x80000, 0x20000 | 0x80000 | 0x800 | 0x100, 0x800 | 0x20000] {
            let arm = translate_linux_open_flags(flags, LinuxOpenAbi::Arm);
            if flags & 0x10000 != 0 {
                assert_eq!(arm & 0x4000, 0x4000, "{flags:#x}");
                assert_eq!(arm & 0x10000, 0, "{flags:#x}");
            }
            if flags & 0x20000 != 0 {
                assert_eq!(arm & 0x8000, 0x8000, "{flags:#x}");
                assert_eq!(arm & 0x20000, 0, "{flags:#x}");
            }
            assert_eq!(arm & 0x80000, flags & 0x80000, "{flags:#x}");
            assert_eq!(arm & 0x800, flags & 0x800, "{flags:#x}");
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn superscript_device_names_are_rejected() {
        use super::{folder_permission, windows_folder_path};
        for digit in ['\u{00B9}', '\u{00B2}', '\u{00B3}'] {
            for stem in ["COM", "com", "LPT", "lpt"] {
                let bare = format!(r"C:\{stem}{digit}");
                let extended = format!(r"C:\dir\{stem}{digit}.txt");
                assert!(!windows_folder_path(&bare), "{bare}");
                assert!(!windows_folder_path(&extended), "{extended}");
                assert!(folder_permission(true, &bare, false).is_err(), "{bare}");
            }
        }
        for digit in ['\u{2074}', '\u{2075}', '\u{2076}', '\u{2077}', '\u{2078}', '\u{2079}'] {
            for stem in ["COM", "LPT"] {
                let bare = format!(r"C:\{stem}{digit}");
                let extended = format!(r"C:\dir\{stem}{digit}.txt");
                assert!(windows_folder_path(&bare), "{bare}");
                assert!(windows_folder_path(&extended), "{extended}");
                assert_eq!(folder_permission(true, &bare, false), Ok(()), "{bare}");
            }
        }
        assert!(windows_folder_path(r"C:\COM10"));
        assert!(windows_folder_path(r"C:\COM0"));
        assert!(windows_folder_path(r"C:\COM0.txt"));
        assert!(windows_folder_path(r"C:\LPT0.txt"));
        assert!(!windows_folder_path(r"C:\COM1"));
        assert!(!windows_folder_path(r"C:\LPT9.dat"));
        #[cfg(windows)]
        {
            use super::widen_desktop_path;
            let probe = format!(r"C:\COM{}", '\u{00B9}');
            let widened = widen_desktop_path(std::path::Path::new(&probe)).unwrap_err();
            assert_eq!(widened.kind(), std::io::ErrorKind::InvalidInput);
            let ordinary = format!(r"C:\COM{}", '\u{2074}');
            assert!(widen_desktop_path(std::path::Path::new(&ordinary)).is_ok());
            assert!(widen_desktop_path(std::path::Path::new(r"C:\COM0.txt")).is_ok());
        }
    }

    #[cfg_attr(not(rllm_module_tests), test)]
    fn all_zero_file_id_is_not_an_identity() {
        assert!(!super::win::file_id_usable(&[0u8; 16]));
        assert!(super::win::extended_descriptor(&[0u8; 16]).is_none());
        let mut id = [0u8; 16];
        id[15] = 1;
        let (kind, data) = super::win::extended_descriptor(&id).unwrap();
        assert_eq!(kind, 2);
        assert_eq!(data, id);
        assert_ne!(kind, 0);
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn identical_visible_files_do_not_share_or_swap_an_identity() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("same-visible");
        let first_path = root.join("a.txt");
        let second_path = root.join("b.txt");
        std::fs::write(&first_path, b"same").unwrap();
        std::fs::write(&second_path, b"same").unwrap();
        let stamp = 0x01DC_1000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (dev_a, ino_a, id_a) = published_identity(&first_file);
        let (dev_b, ino_b, id_b) = published_identity(&second_file);
        assert_ne!(id_a, id_b);
        assert_ne!((dev_a, ino_a), (dev_b, ino_b));
        let first = std::fs::metadata(&first_path).unwrap();
        let second = std::fs::metadata(&second_path).unwrap();
        assert_eq!((first.dev(), first.ino()), (dev_a, ino_a));
        assert_eq!((second.dev(), second.ino()), (dev_b, ino_b));
        assert_ne!((first.dev(), first.ino()), (second.dev(), second.ino()));
        assert_ne!((first.dev(), first.ino()), (0, 0));
        assert_eq!((first.dev(), first.ino()), (first.dev(), first.ino()));
        let again = super::win::unresolved_identity(1, 2);
        assert_ne!(again, (0, 0));
        assert_eq!(again, super::win::unresolved_identity(1, 2));
        assert_ne!(again, super::win::unresolved_identity(1, 3));
        assert_ne!(again, super::win::unresolved_identity(4, 2));
        let reopened = super::win::reopen_extended(os_identity(&first_file).0 as u32, &id_a).unwrap();
        assert_eq!((reopened.0, reopened.1), (dev_a, ino_a));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn windows_mode_follows_the_read_only_attribute() {
        use super::MetadataExt;
        let root = scratch("mode");
        let path = root.join("note.txt");
        std::fs::write(&path, b"note").unwrap();
        let writable = std::fs::metadata(&path).unwrap();
        assert_ne!(writable.mode() & 0o200, 0, "mode {:o}", writable.mode());
        let mut permissions = writable.permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&path, permissions).unwrap();
        let readonly = std::fs::metadata(&path).unwrap();
        assert_eq!(readonly.mode() & 0o222, 0, "mode {:o}", readonly.mode());
        assert_ne!(writable.mode(), readonly.mode());
        let directory = std::fs::metadata(&root).unwrap();
        assert_ne!(directory.mode() & 0o111, 0, "mode {:o}", directory.mode());
    }

    #[cfg(windows)]
    fn grant_read_only(path: &std::path::Path) {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::{null_mut, read_unaligned};
        #[repr(C, align(8))]
        struct Storage([u8; 512]);
        unsafe extern "system" {
            fn CreateFileW(name: *const u16, access: u32, share: u32, sec: *mut std::ffi::c_void, disp: u32, flags: u32, template: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
            fn CloseHandle(handle: isize) -> i32;
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
            fn OpenProcessToken(process: *mut std::ffi::c_void, access: u32, token: *mut *mut std::ffi::c_void) -> i32;
            fn GetTokenInformation(token: *mut std::ffi::c_void, class: i32, info: *mut std::ffi::c_void, len: u32, returned: *mut u32) -> i32;
            fn GetLengthSid(sid: *mut std::ffi::c_void) -> u32;
            fn InitializeAcl(acl: *mut u8, len: u32, rev: u32) -> i32;
            fn AddAccessAllowedAce(acl: *mut u8, rev: u32, mask: u32, sid: *mut std::ffi::c_void) -> i32;
            fn InitializeSecurityDescriptor(sd: *mut u8, rev: u32) -> i32;
            fn SetSecurityDescriptorDacl(sd: *mut u8, present: i32, acl: *mut u8, defaulted: i32) -> i32;
            fn SetSecurityDescriptorControl(sd: *mut u8, interest: u16, bits: u16) -> i32;
            fn SetKernelObjectSecurity(handle: *mut std::ffi::c_void, info: u32, descriptor: *mut std::ffi::c_void) -> i32;
        }
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe { CreateFileW(wide.as_mut_ptr(), 0x0006_0000, 7, null_mut(), 3, 0, null_mut()) };
        assert!(handle != null_mut() && handle != (-1isize as *mut std::ffi::c_void), "{}", std::io::Error::last_os_error());
        let mut token = null_mut();
        assert_ne!(unsafe { OpenProcessToken(GetCurrentProcess(), 0x0008, &mut token) }, 0, "{}", std::io::Error::last_os_error());
        let mut returned = 0u32;
        unsafe { GetTokenInformation(token, 1, null_mut(), 0, &mut returned); }
        let mut raw = vec![0u8; returned as usize];
        assert_ne!(unsafe { GetTokenInformation(token, 1, raw.as_mut_ptr().cast(), returned, &mut returned) }, 0, "{}", std::io::Error::last_os_error());
        let sid = unsafe { read_unaligned(raw.as_ptr().cast::<*mut std::ffi::c_void>()) };
        let sid_len = unsafe { GetLengthSid(sid) };
        let mut acl = Storage([0; 512]);
        assert_ne!(unsafe { InitializeAcl(acl.0.as_mut_ptr(), 16 + sid_len, 2) }, 0, "{}", std::io::Error::last_os_error());
        assert_ne!(unsafe { AddAccessAllowedAce(acl.0.as_mut_ptr(), 2, 0x0012_0089, sid) }, 0, "{}", std::io::Error::last_os_error());
        let mut descriptor = Storage([0; 512]);
        assert_ne!(unsafe { InitializeSecurityDescriptor(descriptor.0.as_mut_ptr(), 1) }, 0, "{}", std::io::Error::last_os_error());
        assert_ne!(unsafe { SetSecurityDescriptorDacl(descriptor.0.as_mut_ptr(), 1, acl.0.as_mut_ptr(), 0) }, 0, "{}", std::io::Error::last_os_error());
        assert_ne!(unsafe { SetSecurityDescriptorControl(descriptor.0.as_mut_ptr(), 0x1000, 0x1000) }, 0, "{}", std::io::Error::last_os_error());
        assert_ne!(unsafe { SetKernelObjectSecurity(handle, 4, descriptor.0.as_mut_ptr().cast()) }, 0, "{}", std::io::Error::last_os_error());
        unsafe { CloseHandle(token as isize); }
        unsafe { CloseHandle(handle as isize); }
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn read_only_acl_reports_the_read_bit_without_write() {
        use super::MetadataExt;
        use std::os::windows::fs::MetadataExt as WinMeta;
        let root = scratch("acl-mode");
        let limited_path = root.join("read-only.txt");
        let open_path = root.join("open.txt");
        std::fs::write(&limited_path, b"read").unwrap();
        std::fs::write(&open_path, b"open").unwrap();
        grant_read_only(&limited_path);
        let limited = std::fs::metadata(&limited_path).unwrap();
        let open = std::fs::metadata(&open_path).unwrap();
        assert_eq!(WinMeta::file_attributes(&limited) & 0x1, 0, "attributes {:x}", WinMeta::file_attributes(&limited));
        assert_ne!(limited.mode() & 0o400, 0, "mode {:o}", limited.mode());
        assert_eq!(limited.mode() & 0o222, 0, "mode {:o}", limited.mode());
        assert_ne!(open.mode() & 0o200, 0, "mode {:o}", open.mode());
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn recycled_metadata_address_keeps_each_file() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("recycled-slot");
        let first_path = root.join("a.bin");
        let second_path = root.join("b.bin");
        std::fs::write(&first_path, b"same").unwrap();
        std::fs::write(&second_path, b"same").unwrap();
        let stamp = 0x01DC_1000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let mut slot = std::fs::metadata(&first_path).unwrap();
        let address_a = &slot as *const std::fs::Metadata as usize;
        let first = (slot.dev(), slot.ino());
        slot = std::fs::metadata(&second_path).unwrap();
        let address_b = &slot as *const std::fs::Metadata as usize;
        let second = (slot.dev(), slot.ino());
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (expected_a, _, _) = published_identity(&first_file);
        let (expected_b_dev, expected_b_ino, _) = published_identity(&second_file);
        let expected_a = (expected_a, published_identity(&first_file).1);
        let expected_b = (expected_b_dev, expected_b_ino);
        assert_eq!(address_a, address_b);
        assert_eq!(first, expected_a);
        assert_eq!(second, expected_b);
        assert_ne!(second, first);
        assert_ne!(second, (0, 0));
        assert_eq!(second, (slot.dev(), slot.ino()));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn listed_directory_does_not_identify_an_outside_file() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("listed-outside");
        let listed_dir = root.join("listed");
        let other_dir = root.join("other");
        std::fs::create_dir(&listed_dir).unwrap();
        std::fs::create_dir(&other_dir).unwrap();
        let listed_path = listed_dir.join("listed.bin");
        let other_path = other_dir.join("target.bin");
        std::fs::write(&listed_path, b"12345678").unwrap();
        std::fs::write(&other_path, b"12345678").unwrap();
        let stamp = 0x01DC_1000_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&listed_path, &other_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        for entry in std::fs::read_dir(&listed_dir).unwrap() {
            let _ = entry.unwrap().file_name();
        }
        let target = std::fs::metadata(&other_path).unwrap();
        let reported = (target.dev(), target.ino());
        let listed_file = std::fs::File::open(&listed_path).unwrap();
        let other_file = std::fs::File::open(&other_path).unwrap();
        let (listed_dev, listed_ino, _) = published_identity(&listed_file);
        let (other_dev, other_ino, _) = published_identity(&other_file);
        assert_ne!((listed_dev, listed_ino), (other_dev, other_ino));
        assert_eq!(reported, (other_dev, other_ino));
        assert_ne!(reported, (listed_dev, listed_ino));
        assert_ne!(reported, (0, 0));
        assert_eq!((target.ctime_nsec(), target.ctime_nsec()), (target.ctime_nsec(), target.ctime_nsec()));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn ordinary_com0_and_later_superscripts_can_be_created() {
        use super::open_at;
        let root = scratch("ordinary-names");
        let parent = open_dir(&root);
        let mut created = open_at(&parent, b"COM0.txt", 1 | 0x40 | 0x80).unwrap();
        use std::io::Write;
        created.write_all(b"ordinary file").unwrap();
        drop(created);
        assert_eq!(std::fs::read(root.join("COM0.txt")).unwrap(), b"ordinary file");
        let later = format!("LPT{}.txt", '\u{2074}');
        let mut created = open_at(&parent, later.as_bytes(), 1 | 0x40 | 0x80).unwrap();
        created.write_all(b"later").unwrap();
        drop(created);
        assert!(root.join(&later).is_file(), "{later}");
        for bad in ["COM1", "COM\u{00B9}", "LPT\u{00B2}.txt", "lpt\u{00B3}"] {
            let err = open_at(&parent, bad.as_bytes(), 1 | 0x40 | 0x80).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{bad}");
            assert!(!root.join(bad).exists(), "{bad}");
        }
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn missing_file_id_uses_the_opened_path_identity() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("opened-path");
        let first_path = root.join("a.txt");
        let second_path = root.join("b.txt");
        std::fs::write(&first_path, b"alpha").unwrap();
        std::fs::write(&second_path, b"beta!!").unwrap();
        let stamp = 0x01DC_2100_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let mut first = None;
        let mut second = None;
        let mut again = None;
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let name = entry.file_name();
            let meta = entry.metadata().unwrap();
            if name == "a.txt" {
                again = Some(entry.metadata().unwrap());
                first = Some(meta);
            } else if name == "b.txt" {
                second = Some(meta);
            }
        }
        let first = first.unwrap();
        let second = second.unwrap();
        let again = again.unwrap();
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (dev_a, ino_a, _) = published_identity(&first_file);
        let (dev_b, ino_b, _) = published_identity(&second_file);
        assert_eq!((first.dev(), first.ino()), (dev_a, ino_a));
        assert_eq!((again.dev(), again.ino()), (dev_a, ino_a));
        assert_eq!((second.dev(), second.ino()), (dev_b, ino_b));
        assert_ne!((dev_a, ino_a), (dev_b, ino_b));
        assert_ne!((dev_a, ino_a), (0, 0));
        let mut slot = first;
        let address_a = &slot as *const std::fs::Metadata as usize;
        let kept = (slot.dev(), slot.ino());
        slot = second;
        let address_b = &slot as *const std::fs::Metadata as usize;
        assert_eq!(address_a, address_b);
        assert_eq!(kept, (dev_a, ino_a));
        assert_eq!((slot.dev(), slot.ino()), (dev_b, ino_b));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn unresolved_file_id_is_unknown_instead_of_an_address() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("unknown-id");
        let first_path = root.join("a.txt");
        let second_path = root.join("b.txt");
        std::fs::write(&first_path, b"same").unwrap();
        std::fs::write(&second_path, b"same").unwrap();
        let stamp = 0x01DC_2200_0000_0000u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        for path in [&first_path, &second_path] {
            let file = std::fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
            assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        }
        let mut saved = Vec::new();
        for entry in std::fs::read_dir(&root).unwrap().flatten() {
            let name = entry.file_name();
            if name == "a.txt" || name == "b.txt" {
                saved.push(entry.metadata().unwrap());
            }
        }
        assert_eq!(saved.len(), 2);
        let first_file = std::fs::File::open(&first_path).unwrap();
        let second_file = std::fs::File::open(&second_path).unwrap();
        let (dev_a, ino_a, _) = published_identity(&first_file);
        let (dev_b, ino_b, _) = published_identity(&second_file);
        assert_ne!((dev_a, ino_a), (dev_b, ino_b));
        assert_ne!((dev_a, ino_a), (0, 0));
        for meta in &saved {
            assert_eq!((meta.dev(), meta.ino()), (0, 0));
            assert_ne!((meta.dev(), meta.ino()), (dev_a, ino_a));
            assert_ne!((meta.dev(), meta.ino()), (dev_b, ino_b));
        }
        let mut slot = saved.pop().unwrap();
        assert_eq!((slot.dev(), slot.ino()), (0, 0));
        slot = saved.pop().unwrap();
        assert_eq!((slot.dev(), slot.ino()), (0, 0));
        let indexed = std::fs::metadata(&first_path).unwrap();
        assert_eq!((indexed.dev(), indexed.ino()), (dev_a, ino_a));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn open_by_index_accepts_only_the_full_volume_and_file_id() {
        let mut id = [0u8; 16];
        id[..8].copy_from_slice(&0x1111_2222_3333_4444u64.to_le_bytes());
        id[8..].copy_from_slice(&0x5555_6666_7777_8888u64.to_le_bytes());
        let mut other = id;
        other[8..].copy_from_slice(&0x9999_aaaa_bbbb_ccccu64.to_le_bytes());
        let volume = 0x0000_0001_0000_0001u64;
        assert!(super::win::same_opened_identity(volume, &id, volume, &id));
        assert!(!super::win::same_opened_identity(volume, &id, volume, &other));
        assert!(!super::win::same_opened_identity(volume, &id, volume as u32 as u64, &id));
        assert!(!super::win::same_opened_identity(volume, &id, volume ^ (1 << 32), &id));
        let mut low_only = [0u8; 16];
        low_only[..8].copy_from_slice(&id[..8]);
        assert!(!super::win::same_opened_identity(volume, &id, volume, &low_only));
    }

    #[cfg(windows)]
    #[cfg_attr(not(rllm_module_tests), test)]
    fn replaced_file_does_not_keep_a_stale_index_identity() {
        use super::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        let root = scratch("stale-index");
        let path = root.join("slot.txt");
        std::fs::write(&path, b"original-bytes").unwrap();
        let stamp = 0x01DC_2300_0000_1111u64;
        unsafe extern "system" {
            fn SetFileTime(handle: *mut std::ffi::c_void, created: *const u64, accessed: *const u64, written: *const u64) -> i32;
        }
        let file = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        assert_ne!(unsafe { SetFileTime(file.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        drop(file);
        let old = std::fs::metadata(&path).unwrap();
        let old_id = (old.dev(), old.ino());
        assert_ne!(old_id, (0, 0));
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"original-bytes").unwrap();
        let created = std::fs::OpenOptions::new().read(true).write(true).open(&path).unwrap();
        assert_ne!(unsafe { SetFileTime(created.as_raw_handle(), &stamp, &stamp, &stamp) }, 0);
        drop(created);
        let new_meta = std::fs::metadata(&path).unwrap();
        let new_id = (new_meta.dev(), new_meta.ino());
        assert_ne!(new_id, (0, 0));
        assert_ne!(new_id, old_id);
        let reported = (old.dev(), old.ino());
        assert_ne!(reported, new_id);
        assert!(reported == (0, 0) || reported == old_id);
    }

    #[cfg(rllm_module_tests)]
    pub fn run() -> usize {
        let mut passed = 0usize;
        folder_permission_uses_the_native_path_format();
        passed += 1;
        macos_open_translates_linux_nofollow_and_create_flags();
        passed += 1;
        oversized_windows_path_is_rejected_before_use();
        passed += 1;
        arm_linux_open_flags_keep_directory_and_nofollow();
        passed += 1;
        superscript_device_names_are_rejected();
        passed += 1;
        all_zero_file_id_is_not_an_identity();
        passed += 1;
        #[cfg(windows)]
        {
            windows_identity_is_volume_serial_and_file_index();
            passed += 1;
            open_at_uses_the_directory_handle();
            passed += 1;
            open_at_ignores_a_swapped_directory_path();
            passed += 1;
            open_at_nofollow_does_not_follow_a_junction();
            passed += 1;
            pre_1970_filetime_keeps_the_negative_second();
            passed += 1;
            high_half_of_file_id_is_part_of_identity();
            passed += 1;
            restored_write_time_does_not_hide_a_change();
            passed += 1;
            index_less_metadata_is_not_the_creation_time();
            passed += 1;
            open_at_rejects_stream_device_and_trailing_names();
            passed += 1;
            directory_metadata_keeps_distinct_os_identities_when_the_address_is_reused();
            passed += 1;
            unclaimed_observation_cache_cannot_grow_without_a_bound();
            passed += 1;
            identical_visible_files_do_not_share_or_swap_an_identity();
            passed += 1;
            windows_mode_follows_the_read_only_attribute();
            passed += 1;
            read_only_acl_reports_the_read_bit_without_write();
            passed += 1;
            recycled_metadata_address_keeps_each_file();
            passed += 1;
            listed_directory_does_not_identify_an_outside_file();
            passed += 1;
            ordinary_com0_and_later_superscripts_can_be_created();
            passed += 1;
            missing_file_id_uses_the_opened_path_identity();
            passed += 1;
            unresolved_file_id_is_unknown_instead_of_an_address();
            passed += 1;
            open_by_index_accepts_only_the_full_volume_and_file_id();
            passed += 1;
            replaced_file_does_not_keep_a_stale_index_identity();
            passed += 1;
            for bad in ["COM\u{00B9}", "LPT\u{00B3}.txt", "com\u{00B2}"] {
                let root = scratch("super-name");
                let parent = open_dir(&root);
                let err = super::open_at(&parent, bad.as_bytes(), 1 | 0x40 | 0x80).unwrap_err();
                assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput, "{bad}");
                assert!(!root.join(bad).exists(), "{bad}");
            }
            let root = scratch("later-super");
            let parent = open_dir(&root);
            let allowed = format!("COM{}.txt", '\u{2079}');
            let mut created = super::open_at(&parent, allowed.as_bytes(), 1 | 0x40 | 0x80).unwrap();
            use std::io::Write;
            created.write_all(b"ok").unwrap();
            drop(created);
            assert!(root.join(&allowed).is_file(), "{allowed}");
            passed += 1;
        }
        passed
    }
}

#[cfg(rllm_module_tests)]
pub fn g1_fix4_host_fs_tests() {
    let passed = checks::run();
    println!("{passed} passed, 0 failed");
}

#[cfg(rllm_module_tests)]
pub fn g1_fix5_host_fs_tests() {
    g1_fix4_host_fs_tests();
}

#[cfg(rllm_module_tests)]
pub fn g1_fix6_host_fs_tests() {
    g1_fix5_host_fs_tests();
}

#[cfg(rllm_module_tests)]
pub fn g1_fix8_host_fs_tests() {
    g1_fix6_host_fs_tests();
}

#[cfg(rllm_module_tests)]
pub fn g1_fix10_host_fs_tests() {
    g1_fix8_host_fs_tests();
}

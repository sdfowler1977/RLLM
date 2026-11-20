//! RLLM for Windows setup. Installs RLLM for the current user without
//! administrator rights: the native desktop build for this PC's processor, Start
//! menu and desktop shortcuts, and an Apps & features entry. Removing RLLM keeps
//! the person's conversations and settings.
#![windows_subsystem = "windows"]
use std::ffi::c_void;
use std::path::{Path, PathBuf};

#[cfg(rllm_arm64)]
static ARM64: Option<&[u8]> = Some(include_bytes!(env!("RLLM_ARM64")));
#[cfg(not(rllm_arm64))]
static ARM64: Option<&[u8]> = None;
static X64: &[u8] = include_bytes!(env!("RLLM_X64"));

const TITLE: &str = "RLLM Setup";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\RLLM";
const MACHINE_ARM64: u16 = 0xAA64;
const MB_OKCANCEL: u32 = 0x1;
const MB_RETRYCANCEL: u32 = 0x5;
const MB_YESNO: u32 = 0x4;
const MB_ICONERROR: u32 = 0x10;
const MB_ICONQUESTION: u32 = 0x20;
const MB_ICONINFORMATION: u32 = 0x40;
const IDOK: i32 = 1;
const IDRETRY: i32 = 4;
const IDYES: i32 = 6;

#[repr(C)]
struct Guid(u32, u16, u16, [u8; 8]);
const FOLDER_DESKTOP: Guid = Guid(0xB4BFCC3A, 0xDB2C, 0x424C, [0xB0, 0x29, 0x7F, 0xE9, 0x9A, 0x87, 0xC6, 0x41]);
const FOLDER_PROGRAMS: Guid = Guid(0xA77F5D77, 0x2E2B, 0x44C3, [0xA6, 0xA2, 0xAB, 0xA6, 0x01, 0x05, 0x4A, 0x51]);
const FOLDER_LOCAL_APP_DATA: Guid = Guid(0xF1B32785, 0x6FBA, 0x4FCF, [0x9D, 0x55, 0x7B, 0x8E, 0x7F, 0x15, 0x70, 0x91]);
const SHELL_LINK: Guid = Guid(0x00021401, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const SHELL_LINK_W: Guid = Guid(0x000214F9, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);
const PERSIST_FILE: Guid = Guid(0x0000010B, 0, 0, [0xC0, 0, 0, 0, 0, 0, 0, 0x46]);

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(owner: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetProcAddress(module: isize, name: *const u8) -> *const c_void;
    fn GetCurrentProcess() -> isize;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn SHGetKnownFolderPath(folder: *const Guid, flags: u32, token: isize, path: *mut *mut u16) -> i32;
    fn ShellExecuteW(owner: isize, verb: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> isize;
    fn SHChangeNotify(event: i32, flags: u32, first: *const c_void, second: *const c_void);
}
#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *const c_void, model: u32) -> i32;
    fn CoCreateInstance(class: *const Guid, outer: isize, context: u32, interface: *const Guid, object: *mut *mut c_void) -> i32;
    fn CoTaskMemFree(memory: *mut c_void);
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn RegCreateKeyExW(key: isize, subkey: *const u16, reserved: u32, class: *const u16, options: u32, access: u32, security: *const c_void, result: *mut isize, disposition: *mut u32) -> i32;
    fn RegSetValueExW(key: isize, name: *const u16, reserved: u32, kind: u32, data: *const u8, size: u32) -> i32;
    fn RegCloseKey(key: isize) -> i32;
    fn RegDeleteKeyW(key: isize, subkey: *const u16) -> i32;
}
const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

fn ask(text: &str, kind: u32) -> i32 {
    unsafe { MessageBoxW(0, wide(text).as_ptr(), wide(TITLE).as_ptr(), kind) }
}

fn known_folder(folder: &Guid) -> Result<PathBuf, String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { SHGetKnownFolderPath(folder, 0, 0, &mut raw) } < 0 || raw.is_null() {
        return Err("Windows did not report a required user folder.".into());
    }
    let mut length = 0;
    while unsafe { *raw.add(length) } != 0 {
        length += 1;
    }
    let path = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(raw, length) });
    unsafe { CoTaskMemFree(raw as *mut c_void) };
    Ok(PathBuf::from(path))
}

/// The processor this PC really has, even when this setup itself runs under x64 emulation.
fn native_machine() -> u16 {
    type IsWow64Process2 = unsafe extern "system" fn(isize, *mut u16, *mut u16) -> i32;
    unsafe {
        let kernel = GetModuleHandleW(wide("kernel32.dll").as_ptr());
        let address = GetProcAddress(kernel, b"IsWow64Process2\0".as_ptr());
        if address.is_null() {
            return 0x8664;
        }
        let query: IsWow64Process2 = std::mem::transmute(address);
        let (mut process, mut native) = (0u16, 0u16);
        if query(GetCurrentProcess(), &mut process, &mut native) != 0 { native } else { 0x8664 }
    }
}

struct Places {
    directory: PathBuf,
    program: PathBuf,
    uninstaller: PathBuf,
    start_menu: PathBuf,
    desktop: PathBuf,
    data: PathBuf,
}

fn places() -> Result<Places, String> {
    let local = known_folder(&FOLDER_LOCAL_APP_DATA)?;
    let directory = local.join("Programs").join("RLLM");
    Ok(Places {
        program: directory.join("RLLM.exe"),
        uninstaller: directory.join("Uninstall RLLM.exe"),
        directory,
        start_menu: known_folder(&FOLDER_PROGRAMS)?.join("RLLM.lnk"),
        desktop: known_folder(&FOLDER_DESKTOP)?.join("RLLM.lnk"),
        data: local.join("rllm"),
    })
}

unsafe fn method<T>(object: *mut c_void, index: usize) -> T {
    let table = unsafe { *(object as *const *const usize) };
    unsafe { std::mem::transmute_copy(&*table.add(index)) }
}

fn shortcut(link: &Path, target: &Path, directory: &Path) -> Result<(), String> {
    type Text = unsafe extern "system" fn(*mut c_void, *const u16) -> i32;
    type Icon = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    type Query = unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
    type Save = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;
    unsafe {
        let mut link_object = std::ptr::null_mut();
        if CoCreateInstance(&SHELL_LINK, 0, 1, &SHELL_LINK_W, &mut link_object) < 0 {
            return Err("Windows could not create a shortcut.".into());
        }
        let target_text = wide_path(target);
        method::<Text>(link_object, 20)(link_object, target_text.as_ptr());
        method::<Text>(link_object, 9)(link_object, wide_path(directory).as_ptr());
        method::<Text>(link_object, 7)(link_object, wide("Chat with your language models").as_ptr());
        method::<Icon>(link_object, 17)(link_object, target_text.as_ptr(), 0);
        let mut file = std::ptr::null_mut();
        let saved = if method::<Query>(link_object, 0)(link_object, &PERSIST_FILE, &mut file) >= 0 {
            let result = method::<Save>(file, 6)(file, wide_path(link).as_ptr(), 1);
            method::<Release>(file, 2)(file);
            result >= 0
        } else {
            false
        };
        method::<Release>(link_object, 2)(link_object);
        if saved { Ok(()) } else { Err(format!("Windows could not save the shortcut {}.", link.display())) }
    }
}

fn register(places: &Places, size: u64) -> Result<(), String> {
    let mut key = 0isize;
    if unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr(), 0, std::ptr::null(), 0, 0x20006, std::ptr::null(), &mut key, std::ptr::null_mut()) } != 0 {
        return Err("Windows did not accept the Apps & features entry.".into());
    }
    let text = |name: &str, value: &str| {
        let data = wide(value);
        unsafe { RegSetValueExW(key, wide(name).as_ptr(), 0, 1, data.as_ptr() as *const u8, (data.len() * 2) as u32) };
    };
    let number = |name: &str, value: u32| unsafe {
        RegSetValueExW(key, wide(name).as_ptr(), 0, 4, &value as *const u32 as *const u8, 4);
    };
    text("DisplayName", "RLLM");
    text("DisplayVersion", "0.1");
    text("Publisher", "RLLM");
    text("DisplayIcon", &format!("{},0", places.program.display()));
    text("InstallLocation", &places.directory.display().to_string());
    text("UninstallString", &format!("\"{}\" --uninstall", places.uninstaller.display()));
    number("NoModify", 1);
    number("NoRepair", 1);
    number("EstimatedSize", (size / 1024) as u32);
    unsafe { RegCloseKey(key) };
    Ok(())
}

/// Writes a file that may be held open by a running RLLM, asking the person to close it.
fn replace(path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let staged = path.with_extension("new");
    std::fs::write(&staged, bytes).map_err(|error| format!("RLLM could not be written to {}: {error}", staged.display()))?;
    loop {
        match std::fs::rename(&staged, path) {
            Ok(()) => return Ok(true),
            Err(_) if ask("RLLM is open. Close RLLM, then choose Retry.", MB_RETRYCANCEL | MB_ICONINFORMATION) == IDRETRY => continue,
            Err(_) => {
                let _ = std::fs::remove_file(&staged);
                return Ok(false);
            }
        }
    }
}

/// Removes helpers left by earlier removals; a running helper cannot delete itself.
fn clear_helpers() {
    let current = std::env::current_exe().ok();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("rllm-uninstall-") && name.ends_with(".exe") && Some(entry.path()) != current {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn install() -> Result<(), String> {
    clear_helpers();
    let places = places()?;
    let (payload, label) = match (native_machine(), ARM64) {
        (MACHINE_ARM64, Some(arm64)) => (arm64, "ARM64"),
        _ => (X64, "x64"),
    };
    let prompt = format!(
        "Install RLLM for Windows ({label}) for this user?\n\nRLLM will be installed in:\n{}\n\nShortcuts are added to the Start menu and the desktop. No administrator rights are needed.",
        places.directory.display()
    );
    if ask(&prompt, MB_OKCANCEL | MB_ICONQUESTION) != IDOK {
        return Ok(());
    }
    std::fs::create_dir_all(&places.directory).map_err(|error| format!("{}: {error}", places.directory.display()))?;
    if !replace(&places.program, payload)? {
        return Ok(());
    }
    let setup = std::env::current_exe().map_err(|error| error.to_string())?;
    if setup != places.uninstaller {
        let bytes = std::fs::read(&setup).map_err(|error| format!("{}: {error}", setup.display()))?;
        replace(&places.uninstaller, &bytes)?;
    }
    shortcut(&places.start_menu, &places.program, &places.directory)?;
    shortcut(&places.desktop, &places.program, &places.directory)?;
    register(&places, payload.len() as u64)?;
    unsafe { SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null()) };
    if ask("RLLM is installed. Open it now?", MB_YESNO | MB_ICONINFORMATION) == IDYES {
        unsafe {
            ShellExecuteW(0, wide("open").as_ptr(), wide_path(&places.program).as_ptr(), std::ptr::null(), wide_path(&places.directory).as_ptr(), 1);
        }
    }
    Ok(())
}

/// Removal runs from a temporary copy, because a program cannot delete its own file.
fn start_uninstall() -> Result<(), String> {
    let places = places()?;
    let prompt = format!(
        "Remove RLLM from this PC?\n\nYour conversations and settings in {} are kept.",
        places.data.display()
    );
    if ask(&prompt, MB_OKCANCEL | MB_ICONQUESTION) != IDOK {
        return Ok(());
    }
    let setup = std::env::current_exe().map_err(|error| error.to_string())?;
    let helper = std::env::temp_dir().join(format!("rllm-uninstall-{}.exe", std::process::id()));
    std::fs::copy(&setup, &helper).map_err(|error| format!("{}: {error}", helper.display()))?;
    std::process::Command::new(&helper).arg("--finish-uninstall").spawn().map_err(|error| error.to_string())?;
    Ok(())
}

fn finish_uninstall() -> Result<(), String> {
    clear_helpers();
    let places = places()?;
    for _ in 0..50 {
        if !places.uninstaller.exists() || std::fs::remove_file(&places.uninstaller).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    while places.program.exists() && std::fs::remove_file(&places.program).is_err() {
        if ask("RLLM is open. Close RLLM, then choose Retry.", MB_RETRYCANCEL | MB_ICONINFORMATION) != IDRETRY {
            return Ok(());
        }
    }
    let _ = std::fs::remove_dir_all(&places.directory);
    let _ = std::fs::remove_file(&places.start_menu);
    let _ = std::fs::remove_file(&places.desktop);
    unsafe {
        RegDeleteKeyW(HKEY_CURRENT_USER, wide(UNINSTALL_KEY).as_ptr());
        SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null());
    }
    ask(&format!("RLLM was removed. Your conversations and settings in {} were kept.", places.data.display()), MB_ICONINFORMATION);
    Ok(())
}

fn main() {
    unsafe { CoInitializeEx(std::ptr::null(), 0x2) };
    let args: Vec<String> = std::env::args().collect();
    let outcome = if args.iter().any(|value| value == "--finish-uninstall") {
        finish_uninstall()
    } else if args.iter().any(|value| value == "--uninstall") {
        start_uninstall()
    } else {
        install()
    };
    if let Err(error) = outcome {
        ask(&error, MB_ICONERROR);
    }
}

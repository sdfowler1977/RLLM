//! The Windows window. The core says what is on screen, `arrange` lays it out for
//! a desktop window, `raster` draws it, and this file owns the window itself:
//! the pixel surface, the text boxes, the pointer and the keyboard.
use super::{advance_shown, get, obj, s, text, Bool, Json, Null, Num, Result, Vault};
use std::collections::BTreeMap;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

#[path = "arrange.rs"]
mod arrange;
#[path = "picture.rs"]
mod picture;
#[path = "raster.rs"]
mod raster;

use arrange::{compose, rows, Item, Pal, Ruler, Scene, View, BODY, CENTER, FACES, FONTS, RIGHT, SPACED, WRAP};
use raster::{over, with_alpha, Canvas, Glyph, Picture};

const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_SETFOCUS: u32 = 0x0007;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETCURSOR: u32 = 0x0020;
const WM_GETMINMAXINFO: u32 = 0x0024;
const WM_SETFONT: u32 = 0x0030;
const WM_SETICON: u32 = 0x0080;
const WM_KEYDOWN: u32 = 0x0100;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_COMMAND: u32 = 0x0111;
const WM_TIMER: u32 = 0x0113;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_XBUTTONUP: u32 = 0x020C;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_DPICHANGED: u32 = 0x02E0;
const WM_APP: u32 = 0x8000;
const WM_SHOWN: u32 = WM_APP + 1;
const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const WS_CHILD: u32 = 0x4000_0000;
const ES_MULTILINE: u32 = 0x0004;
const ES_PASSWORD: u32 = 0x0020;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_AUTOHSCROLL: u32 = 0x0080;
const ES_WANTRETURN: u32 = 0x1000;
const EM_GETLINECOUNT: u32 = 0x00BA;
const EM_SETSEL: u32 = 0x00B1;
const EM_SETLIMITTEXT: u32 = 0x00C5;

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PaintInfo {
    hdc: isize,
    erase: i32,
    area: Rect,
    restore: i32,
    update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image: u32,
    across: i32,
    down: i32,
    used: u32,
    important: u32,
    colors: [u32; 1],
}

#[repr(C)]
struct Track {
    size: u32,
    flags: u32,
    hwnd: isize,
    hover: u32,
}

#[repr(C)]
struct IconInfo {
    icon: i32,
    x: u32,
    y: u32,
    mask: isize,
    color: isize,
}

#[repr(C)]
struct WndClassEx {
    size: u32,
    style: u32,
    wnd_proc: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: isize,
    icon: isize,
    cursor: isize,
    background: isize,
    menu: *const u16,
    class: *const u16,
    small_icon: isize,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point_x: i32,
    point_y: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn BeginPaint(hwnd: isize, paint: *mut PaintInfo) -> isize;
    fn EndPaint(hwnd: isize, paint: *const PaintInfo) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut Rect) -> i32;
    fn DrawTextW(hdc: isize, text: *const u16, len: i32, rect: *mut Rect, format: u32) -> i32;
    fn InvalidateRect(hwnd: isize, rect: *const Rect, erase: i32) -> i32;
    fn PostMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> i32;
    fn SendMessageW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn DefWindowProcW(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn CallWindowProcW(proc: isize, hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    fn PostQuitMessage(code: i32);
    fn SetWindowTextW(hwnd: isize, text: *const u16) -> i32;
    fn GetWindowTextLengthW(hwnd: isize) -> i32;
    fn GetWindowTextW(hwnd: isize, buf: *mut u16, max: i32) -> i32;
    fn MoveWindow(hwnd: isize, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
    fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
    fn SetWindowPos(hwnd: isize, after: isize, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn CreateWindowExW(ex: u32, class: *const u16, title: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: isize, menu: isize, instance: isize, param: *mut u8) -> isize;
    fn DestroyWindow(hwnd: isize) -> i32;
    fn SetFocus(hwnd: isize) -> isize;
    fn SetCapture(hwnd: isize) -> isize;
    fn ReleaseCapture() -> i32;
    fn TrackMouseEvent(track: *mut Track) -> i32;
    fn LoadCursorW(instance: isize, name: *const u16) -> isize;
    fn SetCursor(cursor: isize) -> isize;
    fn SetTimer(hwnd: isize, id: usize, ms: u32, proc: usize) -> usize;
    fn KillTimer(hwnd: isize, id: usize) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, handle: isize) -> isize;
    fn CloseClipboard() -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
    fn SetProcessDpiAwarenessContext(context: isize) -> i32;
    fn RegisterClassExW(class: *const WndClassEx) -> u16;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn GetDpiForWindow(hwnd: isize) -> u32;
    fn SystemParametersInfoW(action: u32, param: u32, out: *mut Rect, ini: u32) -> i32;
    fn CreateIconIndirect(info: *const IconInfo) -> isize;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: isize) -> isize;
    fn CreateDIBSection(hdc: isize, info: *const BitmapInfo, usage: u32, bits: *mut *mut u32, section: isize, offset: u32) -> isize;
    fn CreateBitmap(w: i32, h: i32, planes: u32, bits: u32, data: *const u8) -> isize;
    fn SelectObject(hdc: isize, object: isize) -> isize;
    fn DeleteObject(object: isize) -> i32;
    fn BitBlt(dst: isize, x: i32, y: i32, w: i32, h: i32, src: isize, sx: i32, sy: i32, rop: u32) -> i32;
    fn SetTextColor(hdc: isize, color: u32) -> u32;
    fn SetBkColor(hdc: isize, color: u32) -> u32;
    fn SetBkMode(hdc: isize, mode: i32) -> i32;
    fn CreateSolidBrush(color: u32) -> isize;
    fn CreateFontW(height: i32, width: i32, esc: i32, orient: i32, weight: i32, italic: u32, underline: u32, strike: u32, charset: u32, out: u32, clip: u32, quality: u32, pitch: u32, face: *const u16) -> isize;
    fn GetTextMetricsW(hdc: isize, metrics: *mut [i32; 16]) -> i32;
    fn IntersectClipRect(hdc: isize, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    fn SelectClipRgn(hdc: isize, region: isize) -> i32;
    fn SetTextCharacterExtra(hdc: isize, extra: i32) -> i32;
    fn SetTextAlign(hdc: isize, mode: u32) -> u32;
    fn TextOutW(hdc: isize, x: i32, y: i32, text: *const u16, len: i32) -> i32;
    fn GdiFlush() -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GetLastError() -> u32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(handle: isize) -> *mut u8;
    fn GlobalUnlock(handle: isize) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attribute: u32, data: *const u32, size: u32) -> i32;
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(request: *mut OpenFile) -> i32;
    fn GetSaveFileNameW(request: *mut OpenFile) -> i32;
    fn CommDlgExtendedError() -> u32;
}

/// The system "Open" dialog request (OPENFILENAMEW).
#[repr(C)]
struct OpenFile {
    size: u32,
    owner: isize,
    instance: isize,
    filter: *const u16,
    custom_filter: *mut u16,
    custom_filter_length: u32,
    filter_index: u32,
    file: *mut u16,
    file_length: u32,
    file_title: *mut u16,
    file_title_length: u32,
    initial_directory: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_extension: *const u16,
    custom_data: isize,
    hook: usize,
    template_name: *const u16,
    reserved_pointer: *mut u8,
    reserved: u32,
    flags_extended: u32,
}

/// Whether the window is open, so a file can be asked for.
pub fn showing() -> bool {
    FRAME.load(Ordering::Relaxed) != 0
}

/// What the file dialog is asked to offer.
pub enum Files {
    /// A picture or a text file to attach.
    Attachment,
    /// A picture for the wallpaper.
    Picture,
    /// A saved RLLM document.
    Backup,
}

/// Shows the system "Open" dialog over the window. `Ok(None)` when it is closed
/// without a choice. Safe to call from the worker that runs the core's effects.
pub fn choose(title: &str, files: Files) -> Result<Option<String>> {
    let filter = match files {
        Files::Picture => "Images (PNG, JPEG, WebP, BMP)\0*.png;*.jpg;*.jpeg;*.webp;*.bmp\0All files\0*.*\0\0",
        Files::Backup => "RLLM backup (JSON)\0*.json\0All files\0*.*\0\0",
        Files::Attachment => "Images and text\0*.png;*.jpg;*.jpeg;*.txt;*.md;*.json;*.csv;*.log;*.xml;*.yaml;*.yml;*.toml;*.rs;*.py;*.java;*.c;*.h;*.cpp;*.js;*.ts;*.html;*.css;*.sql;*.ini;*.cfg\0Images (PNG, JPEG)\0*.png;*.jpg;*.jpeg\0All files\0*.*\0\0",
    };
    file_dialog(title, filter, "", false)
}

/// Shows the system "Save as" dialog with a suggested name. `Ok(None)` when it
/// is closed without a choice. Replacing an existing file is confirmed by the
/// dialog itself.
pub fn save(title: &str, name: &str) -> Result<Option<String>> {
    let ending = name.rsplit_once('.').map(|(_, ending)| ending.to_lowercase()).unwrap_or_default();
    let filter = match ending.as_str() {
        "json" => "JSON document\0*.json\0All files\0*.*\0\0".to_string(),
        "zip" => "Zip archive\0*.zip\0All files\0*.*\0\0".to_string(),
        "" => "All files\0*.*\0\0".to_string(),
        other => format!("{} file\0*.{other}\0All files\0*.*\0\0", other.to_uppercase()),
    };
    file_dialog(title, &filter, name, true)
}

fn file_dialog(title: &str, filter: &str, name: &str, saving: bool) -> Result<Option<String>> {
    let frame = FRAME.load(Ordering::Relaxed);
    if frame == 0 {
        return Err("Choosing a file needs the RLLM window".into());
    }
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title = wide(title);
    let mut file = vec![0u16; 4096];
    for (slot, unit) in file.iter_mut().zip(name.encode_utf16().filter(|unit| !matches!(*unit, 0..=31 | 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124)).take(240)) {
        *slot = unit;
    }
    let ending = wide(name.rsplit_once('.').map(|(_, ending)| ending).unwrap_or(""));
    // Explorer style, no read-only box, and the working directory is left
    // alone. Opening needs an existing file; saving asks before replacing one.
    let common = 0x0008_0000 | 0x0000_0800 | 0x0000_0004 | 0x0000_0008;
    let mut request = OpenFile {
        size: std::mem::size_of::<OpenFile>() as u32,
        owner: frame,
        instance: 0,
        filter: filter.as_ptr(),
        custom_filter: std::ptr::null_mut(),
        custom_filter_length: 0,
        filter_index: 1,
        file: file.as_mut_ptr(),
        file_length: file.len() as u32,
        file_title: std::ptr::null_mut(),
        file_title_length: 0,
        initial_directory: std::ptr::null(),
        title: title.as_ptr(),
        flags: common | if saving { 0x0000_0002 } else { 0x0000_1000 },
        file_offset: 0,
        file_extension: 0,
        default_extension: if saving && ending.len() > 1 { ending.as_ptr() } else { std::ptr::null() },
        custom_data: 0,
        hook: 0,
        template_name: std::ptr::null(),
        reserved_pointer: std::ptr::null_mut(),
        reserved: 0,
        flags_extended: 0,
    };
    let chosen = unsafe { if saving { GetSaveFileNameW(&mut request) } else { GetOpenFileNameW(&mut request) } };
    if chosen != 0 {
        let length = file.iter().position(|unit| *unit == 0).unwrap_or(file.len());
        return Ok(Some(String::from_utf16_lossy(&file[..length])));
    }
    match unsafe { CommDlgExtendedError() } {
        0 => Ok(None),
        code => Err(format!("The file dialog could not be shown (code {code:#06x})")),
    }
}

struct App {
    state: Json,
    ui: Json,
    vault: Vault,
    root: String,
    frame: isize,
    draft: isize,
    fields: Vec<(String, isize, bool)>,
    scroll: i32,
    stick: bool,
    modal_scroll: i32,
    page_at: BTreeMap<String, usize>,
    busy: bool,
    tick: u32,
    note: String,
    menu: bool,
    synced_draft: String,
    draft_lines: i32,
    dpi: u32,
    width: i32,
    height: i32,
    scene: Scene,
    pointer: (i32, i32),
    hover: Option<usize>,
    pressed: Option<usize>,
    /// Where the button went down, so a press survives the page being laid out
    /// again underneath it (a reply streaming in while Cancel is clicked).
    press_at: Option<(i32, i32)>,
    tracking: bool,
    fonts: [isize; FONTS],
    lines: [i32; FONTS],
    font_key: String,
    marks: BTreeMap<i32, isize>,
    memo: BTreeMap<(u64, usize, i32), i32>,
    canvas: isize,
    bitmap: isize,
    bits: *mut u32,
    size: (i32, i32),
    backdrop: Vec<u32>,
    backdrop_key: String,
    walls: BTreeMap<String, Vec<Picture>>,
    brush: isize,
    look_key: String,
}

// The window lives on the thread that created it; the lock only guards against
// a handler being entered again while another one is still running.
unsafe impl Send for App {}

struct Finished {
    result: std::result::Result<(Json, Json, Vault), String>,
}

static APP: Mutex<Option<App>> = Mutex::new(None);
static FINISHED: Mutex<Option<Finished>> = Mutex::new(None);
static SHOWN: Mutex<Option<Json>> = Mutex::new(None);
static HINTS: Mutex<BTreeMap<isize, String>> = Mutex::new(BTreeMap::new());
static FRAME: AtomicIsize = AtomicIsize::new(0);
static EDIT_PROC: AtomicIsize = AtomicIsize::new(0);
static FIELD_BRUSH: AtomicIsize = AtomicIsize::new(0);
static HINT_FONT: AtomicIsize = AtomicIsize::new(0);
static INK: AtomicU32 = AtomicU32::new(0x00FF_FAF7);
static FIELD: AtomicU32 = AtomicU32::new(0x0024_1710);
static MUTED: AtomicU32 = AtomicU32::new(0x00C5_BCA2);

fn wide(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value).encode_wide().chain(std::iter::once(0)).collect()
}

fn px(dp: i32, dpi: u32) -> i32 {
    dp * dpi as i32 / 96
}

/// 0xAARRGGBB to the colour order the text calls take.
fn gdi(color: u32) -> u32 {
    ((color >> 16) & 255) | (color & 0xFF00) | ((color & 255) << 16)
}

fn with_app<R>(work: impl FnOnce(&mut App) -> R) -> Option<R> {
    let mut slot = APP.try_lock().ok()?;
    slot.as_mut().map(work)
}

fn glyphs() -> &'static BTreeMap<&'static str, Glyph> {
    static SET: OnceLock<BTreeMap<&'static str, Glyph>> = OnceLock::new();
    SET.get_or_init(|| {
        let mut set = BTreeMap::new();
        for (name, document) in [
            ("actions", include_str!("../res/drawable/actions.xml")),
            ("addition", include_str!("../res/drawable/addition.xml")),
            ("arrow_left", include_str!("../res/drawable/arrow_left.xml")),
            ("arrow_right", include_str!("../res/drawable/arrow_right.xml")),
            ("arrow_up", include_str!("../res/drawable/arrow_up.xml")),
            ("cancellation", include_str!("../res/drawable/cancellation.xml")),
            ("confirmation", include_str!("../res/drawable/confirmation.xml")),
            ("conversation", include_str!("../res/drawable/conversation.xml")),
            ("directory", include_str!("../res/drawable/directory.xml")),
            ("history", include_str!("../res/drawable/history.xml")),
            ("icon", include_str!("../res/drawable/icon.xml")),
            ("message", include_str!("../res/drawable/message.xml")),
            ("paper_plane", include_str!("../res/drawable/paper_plane.xml")),
            ("settings", include_str!("../res/drawable/settings.xml")),
            ("transfer", include_str!("../res/drawable/transfer.xml")),
        ] {
            set.insert(name, raster::glyph(document));
        }
        set
    })
}

struct Gauge<'a> {
    dc: isize,
    fonts: [isize; FONTS],
    lines: [i32; FONTS],
    memo: &'a mut BTreeMap<(u64, usize, i32), i32>,
}

fn digest(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl Ruler for Gauge<'_> {
    fn height(&mut self, font: usize, value: &str, width: i32) -> i32 {
        if value.is_empty() {
            return self.lines[font];
        }
        let key = (digest(value), font, width);
        if let Some(known) = self.memo.get(&key) {
            return *known;
        }
        let letters: Vec<u16> = value.encode_utf16().collect();
        let mut rect = Rect { left: 0, top: 0, right: width.max(1), bottom: 0 };
        unsafe {
            let old = SelectObject(self.dc, self.fonts[font]);
            DrawTextW(self.dc, letters.as_ptr(), letters.len() as i32, &mut rect, 0x400 | 0x10 | 0x800 | 0x2000);
            SelectObject(self.dc, old);
        }
        let tall = rect.bottom.max(self.lines[font]);
        if self.memo.len() > 8192 {
            self.memo.clear();
        }
        self.memo.insert(key, tall);
        tall
    }

    fn width(&mut self, font: usize, value: &str) -> i32 {
        if value.is_empty() {
            return 0;
        }
        let key = (digest(value), font, -1);
        if let Some(known) = self.memo.get(&key) {
            return *known;
        }
        let letters: Vec<u16> = value.encode_utf16().collect();
        // Measured the way it is drawn, so characters the face borrows from
        // another font (arrows, symbols) are counted at their real width.
        let mut rect = Rect { left: 0, top: 0, right: 0, bottom: 0 };
        unsafe {
            let old = SelectObject(self.dc, self.fonts[font]);
            DrawTextW(self.dc, letters.as_ptr(), letters.len() as i32, &mut rect, 0x400 | 0x20 | 0x800);
            SelectObject(self.dc, old);
        }
        let wide = rect.right + 2;
        self.memo.insert(key, wide);
        wide
    }

    fn line(&self, font: usize) -> i32 {
        self.lines[font]
    }
}

fn window_text(hwnd: isize) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if read <= 0 { String::new() } else { String::from_utf16_lossy(&buf[..read as usize]).replace("\r\n", "\n") }
}

fn set_text(hwnd: isize, value: &str) {
    let text = wide(&value.replace("\r\n", "\n").replace('\n', "\r\n"));
    unsafe { SetWindowTextW(hwnd, text.as_ptr()); }
}

fn invalidate(hwnd: isize) {
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0); }
}

fn put(target: &mut Json, key: &str, value: Json) {
    if let super::Obj(fields) = target {
        fields.insert(key.into(), value);
    }
}

fn device() -> Json {
    #[repr(C)]
    struct Memory { length: u32, load: u32, total: u64, avail: u64, page: u64, page_avail: u64, virt: u64, virt_avail: u64, ext: u64 }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GlobalMemoryStatusEx(memory: *mut Memory) -> i32; }
    let mut memory = Memory { length: std::mem::size_of::<Memory>() as u32, load: 0, total: 0, avail: 0, page: 0, page_avail: 0, virt: 0, virt_avail: 0, ext: 0 };
    unsafe { GlobalMemoryStatusEx(&mut memory); }
    let cores = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(4);
    obj(vec![
        ("memory_bytes", Num(memory.total.to_string())),
        ("heap_bytes", Num(memory.total.to_string())),
        ("cores", Num(cores.to_string())),
        ("abi", s(if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" })),
        ("application_id", s("RLLM")),
        ("microphone_granted", Bool(false)),
    ])
}

fn offset_minutes() -> i32 {
    #[repr(C)]
    struct Zone { bias: i32, standard_name: [u16; 32], standard_date: [u16; 8], standard_bias: i32, daylight_name: [u16; 32], daylight_date: [u16; 8], daylight_bias: i32 }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GetTimeZoneInformation(zone: *mut Zone) -> u32; }
    let mut zone = Zone { bias: 0, standard_name: [0; 32], standard_date: [0; 8], standard_bias: 0, daylight_name: [0; 32], daylight_date: [0; 8], daylight_bias: 0 };
    let kind = unsafe { GetTimeZoneInformation(&mut zone) };
    -(zone.bias + if kind == 2 { zone.daylight_bias } else { 0 })
}

/// What the Android layer sends with every action: the draft and each field.
fn inputs_from(app: &App, action: &str) -> Json {
    let mut inputs = obj(vec![("draft", s(&window_text(app.draft)))]);
    for field in rows(get(&app.ui, "modal"), "fields") {
        let key = text(&field, "key");
        match text(&field, "kind").as_str() {
            "readonly" => {}
            "check" => put(&mut inputs, &key, Bool(text(&field, "value") == "true")),
            "choice" => put(&mut inputs, &key, s(&text(&field, "value"))),
            _ => {
                let typed = app.fields.iter().find(|(name, _, _)| name == &key).map(|(_, hwnd, _)| window_text(*hwnd));
                put(&mut inputs, &key, s(&typed.unwrap_or_else(|| text(&field, "value"))));
            }
        }
    }
    if matches!(action, "send" | "retry" | "regenerate") {
        put(&mut inputs, "offset_minutes", Num(offset_minutes().to_string()));
    }
    put(&mut inputs, "device", device());
    inputs
}

fn alter_field(ui: &mut Json, key: &str, change: impl Fn(&str) -> String) {
    let mut modal = get(ui, "modal").clone();
    let mut fields = rows(&modal, "fields");
    for field in &mut fields {
        if text(field, "key") == key {
            let next = change(&text(field, "value"));
            put(field, "value", s(&next));
        }
    }
    put(&mut modal, "fields", super::Arr(fields));
    put(ui, "modal", modal);
}

fn copy(frame: isize, value: &str) -> bool {
    let letters = wide(value);
    unsafe {
        if OpenClipboard(frame) == 0 {
            return false;
        }
        EmptyClipboard();
        let handle = GlobalAlloc(2, letters.len() * 2);
        let mut done = false;
        if handle != 0 {
            let target = GlobalLock(handle);
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(letters.as_ptr() as *const u8, target, letters.len() * 2);
                GlobalUnlock(handle);
                done = SetClipboardData(13, handle) != 0;
            }
        }
        CloseClipboard();
        done
    }
}

fn toast(app: &mut App, message: &str) {
    app.note = message.to_string();
    // Long enough to read: a short note three seconds, a sentence up to eight.
    let shown = (2400 + message.chars().count() as u32 * 45).clamp(3200, 8000);
    unsafe { SetTimer(app.frame, 2, shown, 0); }
}

fn hit_at(app: &App, x: i32, y: i32) -> Option<usize> {
    app.scene.hits.iter().enumerate().rev().find_map(|(index, hit)| {
        let inside = x >= hit.area[0] && y >= hit.area[1] && x < hit.area[0] + hit.area[2] && y < hit.area[1] + hit.area[3];
        let shown = hit.clip.map_or(true, |clip| x >= clip[0] && y >= clip[1] && x < clip[2] && y < clip[3]);
        (inside && shown).then_some(index)
    })
}

fn face(ui: &Json) -> String {
    let font = text(get(ui, "appearance"), "font");
    if font.contains("round") {
        "Candara".into()
    } else if font.contains("serif") && !font.contains("sans") {
        "Georgia".into()
    } else {
        "Segoe UI".into()
    }
}

fn scale(ui: &Json) -> i32 {
    match super::number(get(ui, "appearance"), "scale") {
        0 => 100,
        value => value.clamp(100, 200) as i32,
    }
}

/// Fonts, colours and the title bar follow the appearance the core reports.
fn tune(app: &mut App) {
    let pal = Pal::of(&app.ui);
    INK.store(gdi(pal.ink), Ordering::Relaxed);
    FIELD.store(gdi(pal.field), Ordering::Relaxed);
    MUTED.store(gdi(pal.dim), Ordering::Relaxed);
    let look = format!("{:08x}{:08x}{:08x}", pal.field, pal.surface, pal.ink);
    if look != app.look_key {
        app.look_key = look;
        let fresh = unsafe { CreateSolidBrush(gdi(pal.field)) };
        let old = FIELD_BRUSH.swap(fresh, Ordering::Relaxed);
        if old != 0 {
            unsafe { DeleteObject(old); }
        }
        app.brush = fresh;
        let (dark, caption, letters) = (pal.dark as u32, gdi(pal.surface), gdi(pal.ink));
        unsafe {
            DwmSetWindowAttribute(app.frame, 20, &dark, 4);
            DwmSetWindowAttribute(app.frame, 35, &caption, 4);
            DwmSetWindowAttribute(app.frame, 36, &letters, 4);
            DwmSetWindowAttribute(app.frame, 34, &caption, 4);
        }
    }
    let key = format!("{}|{}|{}", app.dpi, scale(&app.ui), face(&app.ui));
    if key == app.font_key {
        return;
    }
    app.font_key = key;
    let name = wide(&face(&app.ui));
    let zoom = scale(&app.ui);
    for (index, (size, weight)) in FACES.iter().enumerate() {
        if app.fonts[index] != 0 {
            unsafe { DeleteObject(app.fonts[index]); }
        }
        let tall = px(*size, app.dpi) * zoom / 100;
        app.fonts[index] = unsafe { CreateFontW(-tall, 0, 0, 0, *weight, 0, 0, 0, 1, 0, 0, 5, 0, name.as_ptr()) };
        let mut metrics = [0i32; 16];
        unsafe {
            let old = SelectObject(app.canvas, app.fonts[index]);
            GetTextMetricsW(app.canvas, &mut metrics);
            SelectObject(app.canvas, old);
        }
        app.lines[index] = metrics[0].max(tall);
    }
    for (_, font) in std::mem::take(&mut app.marks) {
        unsafe { DeleteObject(font); }
    }
    app.memo.clear();
    HINT_FONT.store(app.fonts[BODY], Ordering::Relaxed);
    unsafe {
        SendMessageW(app.draft, WM_SETFONT, app.fonts[BODY] as usize, 1);
        for (_, hwnd, _) in &app.fields {
            SendMessageW(*hwnd, WM_SETFONT, app.fonts[BODY] as usize, 1);
        }
    }
}

unsafe extern "system" fn edit_proc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize {
    let result = unsafe { CallWindowProcW(EDIT_PROC.load(Ordering::Relaxed), hwnd, msg, wparam, lparam) };
    if msg == WM_PAINT && unsafe { GetWindowTextLengthW(hwnd) } == 0 {
        let hint = HINTS.try_lock().ok().and_then(|hints| hints.get(&hwnd).cloned()).unwrap_or_default();
        if !hint.is_empty() {
            let letters: Vec<u16> = hint.encode_utf16().collect();
            let mut area = Rect { left: 0, top: 0, right: 0, bottom: 0 };
            unsafe {
                GetClientRect(hwnd, &mut area);
                area.left += 1;
                let dc = GetDC(hwnd);
                let old = SelectObject(dc, HINT_FONT.load(Ordering::Relaxed));
                SetBkMode(dc, 1);
                SetTextColor(dc, MUTED.load(Ordering::Relaxed));
                DrawTextW(dc, letters.as_ptr(), letters.len() as i32, &mut area, 0x20 | 0x800 | 0x8000);
                SelectObject(dc, old);
                ReleaseDC(hwnd, dc);
            }
        }
    }
    result
}

fn make_edit(app: &App, multiline: bool, secret: bool, hint: &str) -> isize {
    let class = wide("EDIT");
    let empty = wide("");
    let style = WS_CHILD | if multiline { ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN } else { ES_AUTOHSCROLL } | if secret { ES_PASSWORD } else { 0 };
    let hwnd = unsafe { CreateWindowExW(0, class.as_ptr(), empty.as_ptr(), style, 0, 0, 10, 10, app.frame, 0, GetModuleHandleW(std::ptr::null()), std::ptr::null_mut()) };
    if hwnd == 0 {
        return 0;
    }
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, app.fonts[BODY] as usize, 1);
        SendMessageW(hwnd, EM_SETLIMITTEXT, 4_000_000, 0);
        let original = SetWindowLongPtrW(hwnd, -4, edit_proc as *const () as usize as isize);
        EDIT_PROC.store(original, Ordering::Relaxed);
    }
    if !hint.is_empty() {
        if let Ok(mut hints) = HINTS.lock() {
            hints.insert(hwnd, hint.to_string());
        }
    }
    hwnd
}

fn drop_fields(app: &mut App) {
    for (_, hwnd, _) in app.fields.drain(..) {
        if let Ok(mut hints) = HINTS.lock() {
            hints.remove(&hwnd);
        }
        unsafe { DestroyWindow(hwnd); }
    }
}

/// Arrange the scene for the current size and move the text boxes onto it.
fn place(app: &mut App) {
    let mut memo = std::mem::take(&mut app.memo);
    let page = text(&app.ui, "page");
    let mut laid = Scene::default();
    for _ in 0..2 {
        let mut gauge = Gauge { dc: app.canvas, fonts: app.fonts, lines: app.lines, memo: &mut memo };
        let view = View {
            ui: &app.ui,
            profile_form: text(&app.state, "dialog") == "profile",
            width: app.width,
            height: app.height,
            dpi: app.dpi,
            scroll: app.scroll,
            stick: app.stick,
            modal_scroll: app.modal_scroll,
            page_at: app.page_at.get(&page).copied().unwrap_or(0),
            draft_lines: app.draft_lines,
            working: app.busy && app.tick >= 4,
            menu: app.menu,
            note: &app.note,
        };
        laid = compose(&view, &mut gauge);
        let settled = laid.scroll == app.scroll;
        app.scroll = laid.scroll;
        if settled {
            break;
        }
    }
    app.memo = memo;
    app.modal_scroll = app.modal_scroll.clamp(0, laid.modal_span);
    unsafe {
        match laid.draft {
            Some(area) => {
                MoveWindow(app.draft, area[0], area[1], area[2], area[3], 1);
                ShowWindow(app.draft, 8);
            }
            None => {
                ShowWindow(app.draft, 0);
            }
        }
    }
    for field in &laid.fields {
        let known = app.fields.iter().find(|(key, _, _)| key == &field.key).map(|(_, hwnd, _)| *hwnd);
        let hwnd = match known {
            Some(hwnd) => hwnd,
            None => {
                let hwnd = make_edit(app, field.multiline, field.secret, &field.hint);
                if hwnd == 0 {
                    continue;
                }
                set_text(hwnd, &field.value);
                app.fields.push((field.key.clone(), hwnd, field.multiline));
                hwnd
            }
        };
        unsafe {
            MoveWindow(hwnd, field.area[0], field.area[1], field.area[2], field.area[3], 1);
            ShowWindow(hwnd, 8);
        }
    }
    for (key, hwnd, _) in &app.fields {
        if !laid.fields.iter().any(|field| &field.key == key) {
            unsafe { ShowWindow(*hwnd, 0); }
        }
    }
    app.scene = laid;
    app.hover = hit_at(app, app.pointer.0, app.pointer.1);
    app.pressed = app.press_at.and_then(|at| hit_at(app, at.0, at.1));
    if app.pointer.0 >= 0 {
        // The page changed under a resting pointer: show the right cursor now
        // rather than at the next mouse move.
        let hand = app.hover.and_then(|index| app.scene.hits.get(index)).map(|hit| !hit.action.is_empty()).unwrap_or(false);
        unsafe { SetCursor(LoadCursorW(0, if hand { 32649 } else { 32512 } as *const u16)); }
    }
}

fn surface(app: &mut App, width: i32, height: i32) {
    if app.size == (width, height) && app.bitmap != 0 {
        return;
    }
    let info = BitmapInfo { size: 40, width, height: -height, planes: 1, bits: 32, compression: 0, image: 0, across: 0, down: 0, used: 0, important: 0, colors: [0] };
    let mut bits: *mut u32 = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(app.canvas, &info, 0, &mut bits, 0, 0) };
    if bitmap == 0 || bits.is_null() {
        return;
    }
    unsafe {
        SelectObject(app.canvas, bitmap);
        if app.bitmap != 0 {
            DeleteObject(app.bitmap);
        }
    }
    app.bitmap = bitmap;
    app.bits = bits;
    app.size = (width, height);
}

/// The wallpaper, cropped to the window, under the veil: the Android scene.
fn backdrop(app: &mut App, pal: &Pal) {
    let (width, height) = (app.size.0 as usize, app.size.1 as usize);
    let look = get(&app.ui, "appearance");
    let mode = match text(look, "background") {
        value if value.is_empty() => "waves".to_string(),
        value => value,
    };
    let chosen = text(look, "wallpaper");
    let key = format!("{width}x{height}|{mode}|{chosen}|{:08x}|{:08x}|{}", pal.surface, pal.veil, pal.dark);
    if key == app.backdrop_key && app.backdrop.len() == width * height {
        return;
    }
    app.backdrop_key = key;
    app.backdrop.clear();
    app.backdrop.resize(width * height, pal.surface & 0x00FF_FFFF);
    if mode == "solid" {
        return;
    }
    let name = if mode == "image" && !chosen.is_empty() { chosen.clone() } else if pal.dark { "dark".to_string() } else { "light".to_string() };
    if !app.walls.contains_key(&name) {
        let loaded = if name == "dark" || name == "light" { picture::bundled(pal.dark) } else { picture::from_file(chosen.trim_start_matches("file:///")) };
        let levels = match loaded {
            Ok(full) => {
                let mut levels = vec![full];
                while levels.last().map(|level| level.width > 640 && level.height > 360).unwrap_or(false) {
                    let next = levels[levels.len() - 1].half();
                    levels.push(next);
                }
                levels
            }
            Err(error) => {
                toast(app, &format!("The wallpaper could not be shown: {error}"));
                Vec::new()
            }
        };
        app.walls.insert(name.clone(), levels);
    }
    let Some(levels) = app.walls.get(&name) else { return };
    let Some(level) = levels.iter().rev().find(|level| level.width >= width && level.height >= height).or(levels.first()) else { return };
    level.cover(&mut app.backdrop, width, height);
    let veil = pal.veil;
    for pixel in &mut app.backdrop {
        *pixel = over(*pixel, veil, veil >> 24);
    }
}

fn mark_font(app: &mut App, size: i32) -> isize {
    if let Some(font) = app.marks.get(&size) {
        return *font;
    }
    let name = wide(&face(&app.ui));
    let font = unsafe { CreateFontW(-size, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, name.as_ptr()) };
    app.marks.insert(size, font);
    font
}

fn paint(app: &mut App) {
    let mut info = PaintInfo { hdc: 0, erase: 0, area: Rect { left: 0, top: 0, right: 0, bottom: 0 }, restore: 0, update: 0, reserved: [0; 32] };
    let hdc = unsafe { BeginPaint(app.frame, &mut info) };
    let (width, height) = (app.width.max(1), app.height.max(1));
    surface(app, width, height);
    if app.bits.is_null() || app.size != (width, height) {
        unsafe { EndPaint(app.frame, &info); }
        return;
    }
    let pal = Pal::of(&app.ui);
    backdrop(app, &pal);
    let pixels = unsafe { std::slice::from_raw_parts_mut(app.bits, (width * height) as usize) };
    pixels.copy_from_slice(&app.backdrop);
    let mut canvas = Canvas::new(pixels, width, height);
    let dc = app.canvas;
    let line = px(1, app.dpi).max(1) as f32;
    unsafe {
        SetBkMode(dc, 1);
        SelectClipRgn(dc, 0);
    }
    let items = std::mem::take(&mut app.scene.items);
    for item in &items {
        match item {
            Item::Fill { area, radius, color } => canvas.round(area[0], area[1], area[2], area[3], *radius as f32, *color),
            Item::Edge { area, radius, color } => canvas.ring(area[0], area[1], area[2], area[3], *radius as f32, line, *color),
            Item::Icon { x, y, size, name, color } => {
                if let Some(glyph) = glyphs().get(name.as_str()) {
                    canvas.glyph(glyph, *x as f32, *y as f32, *size as f32, if *color == 0 { None } else { Some(*color) });
                }
            }
            Item::Clip(area) => {
                canvas.set_clip(*area);
                unsafe {
                    SelectClipRgn(dc, 0);
                    if let Some(area) = area {
                        IntersectClipRect(dc, area[0], area[1], area[2], area[3]);
                    }
                }
            }
            Item::Hot(index) => {
                if app.hover == Some(*index) {
                    if let Some(hit) = app.scene.hits.get(*index) {
                        let strength = if app.pressed == Some(*index) { 0x38 } else { 0x1E };
                        canvas.round(hit.area[0], hit.area[1], hit.area[2], hit.area[3], hit.radius as f32, with_alpha(pal.ink, strength));
                    }
                }
            }
            Item::Line { from, to, width, color } => canvas.strokes(&[[from.0, from.1, to.0, to.1]], *width, *color),
            Item::Arc { center, radius, start, sweep, width, color } => canvas.arc(center.0, center.1, *radius, *start, *sweep, *width, *color),
            Item::Disc { center, radius, color } => canvas.disc(center.0, center.1, *radius, *color),
            Item::Busy { area, color } => {
                canvas.round(area[0], area[1], area[2], area[3], area[3] as f32 / 2.0, with_alpha(*color, 0x30));
                let part = area[2] / 4;
                let travel = area[2] + part;
                let at = (app.tick as i32 * px(14, app.dpi)) % travel.max(1) - part;
                let (from, to) = (at.max(0), (at + part).min(area[2]));
                if to > from {
                    canvas.round(area[0] + from, area[1], to - from, area[3], area[3] as f32 / 2.0, *color);
                }
            }
            Item::Text { area, font, color, value, mode } => {
                let letters: Vec<u16> = value.encode_utf16().collect();
                let mut rect = Rect { left: area[0], top: area[1], right: area[0] + area[2], bottom: area[1] + area[3] };
                let align = if mode & CENTER != 0 { 1 } else if mode & RIGHT != 0 { 2 } else { 0 };
                let format = if mode & WRAP != 0 { 0x10 | 0x800 | 0x2000 | align } else { 0x20 | 0x4 | 0x800 | 0x8000 | align };
                unsafe {
                    let old = SelectObject(dc, app.fonts[*font]);
                    SetTextColor(dc, gdi(*color));
                    if mode & SPACED != 0 {
                        SetTextCharacterExtra(dc, px(2, app.dpi).max(1));
                    }
                    DrawTextW(dc, letters.as_ptr(), letters.len() as i32, &mut rect, format);
                    if mode & SPACED != 0 {
                        SetTextCharacterExtra(dc, 0);
                    }
                    SelectObject(dc, old);
                    GdiFlush();
                }
            }
            Item::Mark { x, y, size, color, align, value } => {
                let font = mark_font(app, size.round() as i32);
                let letters: Vec<u16> = value.encode_utf16().collect();
                unsafe {
                    let old = SelectObject(dc, font);
                    SetTextColor(dc, gdi(*color));
                    SetTextAlign(dc, 24 | if align & CENTER != 0 { 6 } else if align & RIGHT != 0 { 2 } else { 0 });
                    TextOutW(dc, x.round() as i32, y.round() as i32, letters.as_ptr(), letters.len() as i32);
                    SetTextAlign(dc, 0);
                    SelectObject(dc, old);
                    GdiFlush();
                }
            }
        }
    }
    app.scene.items = items;
    unsafe {
        SelectClipRgn(dc, 0);
        BitBlt(hdc, 0, 0, width, height, dc, 0, 0, 0x00CC_0020);
        EndPaint(app.frame, &info);
    }
}

fn identity(ui: &Json) -> String {
    let modal = get(ui, "modal");
    if modal == &Null {
        return String::new();
    }
    let keys: Vec<String> = rows(modal, "fields").iter().map(|field| text(field, "key")).collect();
    format!("{}|{}", text(modal, "title"), keys.join(","))
}

fn draft_lines(app: &App) -> i32 {
    (unsafe { SendMessageW(app.draft, EM_GETLINECOUNT, 0, 0) } as i32).clamp(1, 6)
}

/// The title bar: the product name and what is open, capitalised.
fn caption_for(ui: &Json) -> String {
    let name = text(ui, "title");
    let mut letters = name.chars();
    let name = match letters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + letters.as_str(),
        None => String::new(),
    };
    if name.is_empty() || name == "RLLM" { "RLLM".to_string() } else { format!("RLLM · {name}") }
}

/// Take a surface the core rendered. A settled surface ends the action; one
/// shown on the way (the turn being sent, the reply arriving) does not.
fn adopt(app: &mut App, ui: Json, settled: bool) {
    let before = (text(&app.ui, "page"), text(&app.ui, "conversation_id"), rows(&app.ui, "messages").len(), identity(&app.ui));
    let draft = text(&ui, "draft");
    app.ui = ui;
    if draft != app.synced_draft {
        set_text(app.draft, &draft);
        app.synced_draft = draft;
    }
    let page = text(&app.ui, "page");
    let messages = rows(&app.ui, "messages");
    if page != before.0 || text(&app.ui, "conversation_id") != before.1 {
        app.scroll = 0;
        app.stick = true;
    }
    if messages.len() > before.2 && messages.last().map(|message| text(message, "role") == "user").unwrap_or(false) {
        app.stick = true;
    }
    let dialog = identity(&app.ui);
    let changed = dialog != before.3;
    if settled || changed {
        drop_fields(app);
    }
    if changed {
        app.modal_scroll = 0;
    }
    app.menu = false;
    tune(app);
    let title = wide(&caption_for(&app.ui));
    unsafe { SetWindowTextW(app.frame, title.as_ptr()); }
    place(app);
    app.draft_lines = draft_lines(app);
    place(app);
    if settled {
        unsafe {
            if !dialog.is_empty() {
                if let Some((_, hwnd, _)) = app.fields.first() {
                    if changed {
                        SetFocus(*hwnd);
                    }
                } else {
                    SetFocus(app.frame);
                }
            } else if page == "chat" {
                SetFocus(app.draft);
            }
        }
    }
    invalidate(app.frame);
}

fn launch(action: String) {
    let prepared = with_app(|app| {
        if app.busy {
            if action == "cancel" {
                super::CANCEL.store(true, Ordering::Relaxed);
                toast(app, "Cancelling…");
                place(app);
                invalidate(app.frame);
            }
            return None;
        }
        let (name, extra) = match action.split_once('#') {
            Some((name, extra)) => (name.to_string(), extra.to_string()),
            None => (action.clone(), String::new()),
        };
        let mut inputs = inputs_from(app, &name);
        for pair in extra.split('&') {
            if let Some((key, value)) = pair.split_once('=') {
                put(&mut inputs, key, if value.parse::<i64>().is_ok() { Num(value.to_string()) } else { s(value) });
            }
        }
        super::CANCEL.store(false, Ordering::Relaxed);
        app.busy = true;
        app.tick = 0;
        app.menu = false;
        app.synced_draft = window_text(app.draft);
        unsafe { SetTimer(app.frame, 1, 40, 0); }
        Some((app.state.clone(), Vault { path: app.vault.path.clone(), key: app.vault.key }, app.root.clone(), inputs, name))
    })
    .flatten();
    let Some((mut state, vault, root, inputs, name)) = prepared else { return };
    std::thread::spawn(move || {
        let frame = FRAME.load(Ordering::Relaxed);
        let outcome = advance_shown(&mut state, &root, &name, inputs, &mut |ui| {
            if let Ok(mut slot) = SHOWN.lock() {
                *slot = Some(ui.clone());
            }
            unsafe { PostMessageW(frame, WM_SHOWN, 0, 0); }
        });
        let result = outcome.and_then(|ui| vault.save(&state).map(|()| (state, ui, vault)));
        if let Ok(mut slot) = FINISHED.lock() {
            *slot = Some(Finished { result });
        }
        unsafe { PostMessageW(frame, WM_APP, 0, 0); }
    });
}

fn finish() {
    let finished = match FINISHED.lock() {
        Ok(mut slot) => slot.take(),
        Err(_) => None,
    };
    let Some(finished) = finished else { return };
    if let Ok(mut slot) = SHOWN.lock() {
        *slot = None;
    }
    with_app(|app| {
        app.busy = false;
        app.tick = 0;
        unsafe { KillTimer(app.frame, 1); }
        match finished.result {
            Ok((state, ui, vault)) => {
                let before = text(&app.state, "notice");
                let said = text(&state, "notice");
                app.state = state;
                app.vault = vault;
                // What the core reports about a finished action (a file saved,
                // an export verified) is said once when no page is showing it.
                let silent = text(&app.ui, "visible_status").is_empty() && text(&ui, "visible_status").is_empty();
                if said != before && !said.is_empty() && !said.starts_with("Ready") && silent {
                    toast(app, &said);
                }
                adopt(app, ui, true);
            }
            Err(error) => {
                toast(app, &error);
                place(app);
                invalidate(app.frame);
            }
        }
    });
}

/// Actions the window settles itself; everything else goes to the core.
fn activate(action: String) {
    if action.is_empty() {
        return;
    }
    let own = matches!(action.as_str(), "menu:open" | "menu:close" | "latest" | "page:prev" | "page:next") || action.starts_with("check:") || action.starts_with("choice:");
    if !own {
        launch(action);
        return;
    }
    with_app(|app| {
        match action.as_str() {
            "menu:open" => app.menu = !app.menu,
            "menu:close" => app.menu = false,
            "latest" => app.stick = true,
            "page:prev" | "page:next" => {
                let page = text(&app.ui, "page");
                let size = app.scene.page_size.max(1);
                let count = rows(&app.ui, "rows").len();
                let current = app.page_at.get(&page).copied().unwrap_or(0) / size * size;
                let next = if action == "page:prev" { current.saturating_sub(size) } else if current + size < count { current + size } else { current };
                app.page_at.insert(page, next);
            }
            _ => {
                if let Some(key) = action.strip_prefix("check:") {
                    alter_field(&mut app.ui, key, |value| if value == "true" { "false".into() } else { "true".into() });
                } else if let Some((key, value)) = action.strip_prefix("choice:").and_then(|rest| rest.split_once(':')) {
                    alter_field(&mut app.ui, key, |_| value.to_string());
                }
            }
        }
        place(app);
        invalidate(app.frame);
    });
}

unsafe extern "system" fn wndproc(hwnd: isize, msg: u32, wparam: usize, lparam: isize) -> isize {
    let point = ((lparam & 0xFFFF) as i16 as i32, ((lparam >> 16) & 0xFFFF) as i16 as i32);
    match msg {
        WM_APP => {
            finish();
            return 0;
        }
        WM_SHOWN => {
            let shown = match SHOWN.lock() {
                Ok(mut slot) => slot.take(),
                Err(_) => None,
            };
            if let Some(ui) = shown {
                with_app(|app| {
                    if app.busy {
                        adopt(app, ui, false);
                    }
                });
            }
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_SIZE => {
            if wparam != 1 {
                with_app(|app| {
                    app.width = point.0.max(1);
                    app.height = point.1.max(1);
                    place(app);
                });
                invalidate(hwnd);
            }
            return 0;
        }
        WM_PAINT => {
            if with_app(paint).is_none() {
                let mut info = PaintInfo { hdc: 0, erase: 0, area: Rect { left: 0, top: 0, right: 0, bottom: 0 }, restore: 0, update: 0, reserved: [0; 32] };
                unsafe {
                    BeginPaint(hwnd, &mut info);
                    EndPaint(hwnd, &info);
                }
            }
            return 0;
        }
        WM_TIMER => {
            with_app(|app| {
                if wparam == 1 && app.busy {
                    app.tick += 1;
                    if app.tick == 4 {
                        place(app);
                    }
                    if app.tick >= 4 {
                        invalidate(app.frame);
                    }
                } else if wparam == 2 {
                    unsafe { KillTimer(app.frame, 2); }
                    app.note.clear();
                    place(app);
                    invalidate(app.frame);
                }
            });
            return 0;
        }
        WM_MOUSEMOVE => {
            with_app(|app| {
                app.pointer = point;
                let now = hit_at(app, point.0, point.1);
                if now != app.hover {
                    app.hover = now;
                    invalidate(app.frame);
                }
                if !app.tracking {
                    let mut track = Track { size: std::mem::size_of::<Track>() as u32, flags: 2, hwnd, hover: 0 };
                    app.tracking = unsafe { TrackMouseEvent(&mut track) } != 0;
                }
            });
            return 0;
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                app.tracking = false;
                app.pointer = (-1, -1);
                if app.hover.take().is_some() {
                    invalidate(app.frame);
                }
            });
            return 0;
        }
        WM_SETCURSOR => {
            if wparam as isize == hwnd && (lparam & 0xFFFF) == 1 {
                let hand = with_app(|app| app.hover.and_then(|index| app.scene.hits.get(index)).map(|hit| !hit.action.is_empty()).unwrap_or(false)).unwrap_or(false);
                unsafe { SetCursor(LoadCursorW(0, if hand { 32649 } else { 32512 } as *const u16)); }
                return 1;
            }
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                app.press_at = Some(point);
                app.pressed = hit_at(app, point.0, point.1);
                invalidate(app.frame);
            });
            unsafe { SetCapture(hwnd); }
            return 0;
        }
        WM_LBUTTONUP => {
            unsafe { ReleaseCapture(); }
            let action = with_app(|app| {
                let now = hit_at(app, point.0, point.1);
                let fire = if now.is_some() && now == app.pressed { now.and_then(|index| app.scene.hits.get(index)).map(|hit| hit.action.clone()) } else { None };
                app.pressed = None;
                app.press_at = None;
                invalidate(app.frame);
                fire
            })
            .flatten();
            if let Some(action) = action {
                activate(action);
            }
            return 0;
        }
        WM_RBUTTONUP => {
            with_app(|app| {
                let index = hit_at(app, point.0, point.1).and_then(|index| app.scene.hits.get(index)).and_then(|hit| hit.action.strip_prefix("branch:").and_then(|value| value.parse::<usize>().ok()));
                if let Some(message) = index.and_then(|index| rows(&app.ui, "messages").get(index).cloned()) {
                    let done = copy(app.frame, &text(&message, "content"));
                    toast(app, if done { "Message copied" } else { "The clipboard is busy; the message was not copied" });
                    place(app);
                    invalidate(app.frame);
                }
            });
            return 0;
        }
        WM_MOUSEWHEEL => {
            with_app(|app| {
                let delta = ((wparam >> 16) as i16 as i32) * px(64, app.dpi) / 120;
                if get(&app.ui, "modal") != &Null {
                    app.modal_scroll = (app.modal_scroll - delta).clamp(0, app.scene.modal_span);
                } else if app.scene.view.is_some() {
                    app.stick = false;
                    app.scroll = (app.scroll - delta).clamp(0, app.scene.content);
                    if text(&app.ui, "page") == "chat" && app.scroll >= app.scene.content {
                        app.stick = true;
                    }
                }
                place(app);
                invalidate(app.frame);
            });
            return 0;
        }
        WM_COMMAND => {
            if (wparam >> 16) & 0xFFFF == 0x0300 && lparam != 0 {
                if unsafe { GetWindowTextLengthW(lparam) } == 0 {
                    invalidate(lparam);
                }
                with_app(|app| {
                    if lparam == app.draft {
                        let lines = draft_lines(app);
                        if lines != app.draft_lines {
                            app.draft_lines = lines;
                            place(app);
                            invalidate(app.frame);
                        }
                    }
                });
            }
            return 0;
        }
        WM_CTLCOLOREDIT => {
            unsafe {
                SetTextColor(wparam as isize, INK.load(Ordering::Relaxed));
                SetBkColor(wparam as isize, FIELD.load(Ordering::Relaxed));
            }
            return FIELD_BRUSH.load(Ordering::Relaxed);
        }
        WM_SETFOCUS => {
            with_app(|app| {
                if get(&app.ui, "modal") == &Null && app.scene.draft.is_some() {
                    unsafe { SetFocus(app.draft); }
                }
            });
            return 0;
        }
        WM_GETMINMAXINFO => {
            let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
            let info = lparam as *mut [i32; 10];
            if !info.is_null() {
                unsafe {
                    (*info)[6] = px(720, dpi);
                    (*info)[7] = px(540, dpi);
                }
            }
            return 0;
        }
        WM_DPICHANGED => {
            let area = lparam as *const Rect;
            with_app(|app| {
                app.dpi = ((wparam >> 16) & 0xFFFF) as u32;
                tune(app);
            });
            if !area.is_null() {
                unsafe { SetWindowPos(hwnd, 0, (*area).left, (*area).top, (*area).right - (*area).left, (*area).bottom - (*area).top, 0x14); }
            }
            return 0;
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0); }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Keys the window handles before a text box sees them. Returns true when the
/// key was used.
fn key(message: &Msg) -> bool {
    let back = (message.message == WM_SYSKEYDOWN && message.wparam == 0x25) || (message.message == WM_XBUTTONUP && (message.wparam >> 16) & 0xFFFF == 1);
    if back {
        if with_app(|app| super::boolean(&app.ui, "can_navigate_back") || get(&app.ui, "modal") != &Null).unwrap_or(false) {
            activate("back".into());
        }
        return true;
    }
    if message.message != WM_KEYDOWN {
        return false;
    }
    let shift = unsafe { GetKeyState(0x10) } < 0;
    let control = unsafe { GetKeyState(0x11) } < 0;
    let Some((draft, dialog, menu, fields, primary, can_back, shown)) = with_app(|app| {
        (
            app.draft,
            get(&app.ui, "modal") != &Null,
            app.menu,
            app.fields.clone(),
            app.scene.primary.clone(),
            super::boolean(&app.ui, "can_navigate_back"),
            app.scene.fields.iter().map(|field| field.key.clone()).collect::<Vec<String>>(),
        )
    }) else {
        return false;
    };
    let field = fields.iter().find(|(_, hwnd, _)| *hwnd == message.hwnd);
    match message.wparam {
        0x0D => {
            if message.hwnd == draft && !shift && !control {
                if !window_text(draft).trim().is_empty() {
                    activate("send".into());
                }
                return true;
            }
            if let Some((_, _, multiline)) = field {
                if !multiline {
                    activate(primary);
                    return true;
                }
            }
            false
        }
        0x1B => {
            if menu {
                activate("menu:close".into());
            } else if dialog {
                activate("close".into());
            } else if can_back {
                activate("back".into());
            }
            true
        }
        0x09 => {
            let visible: Vec<isize> = fields.iter().filter(|(name, _, _)| shown.contains(name)).map(|(_, hwnd, _)| *hwnd).collect();
            if !dialog || visible.is_empty() {
                return false;
            }
            let at = visible.iter().position(|hwnd| *hwnd == message.hwnd);
            let next = match at {
                Some(at) if shift => (at + visible.len() - 1) % visible.len(),
                Some(at) => (at + 1) % visible.len(),
                None => 0,
            };
            unsafe {
                SetFocus(visible[next]);
                SendMessageW(visible[next], EM_SETSEL, 0, -1);
            }
            true
        }
        0x41 if control && (message.hwnd == draft || field.is_some()) => {
            unsafe { SendMessageW(message.hwnd, EM_SETSEL, 0, -1); }
            true
        }
        _ => false,
    }
}

/// The launcher drawable as a window icon.
fn emblem(size: i32) -> isize {
    let Some(glyph) = glyphs().get("icon") else { return 0 };
    let count = (size * size) as usize;
    let mut dark = vec![0u32; count];
    let mut light = vec![0x00FF_FFFFu32; count];
    Canvas::new(&mut dark, size, size).glyph(glyph, 0.0, 0.0, size as f32, None);
    Canvas::new(&mut light, size, size).glyph(glyph, 0.0, 0.0, size as f32, None);
    let pixels: Vec<u32> = dark
        .iter()
        .zip(&light)
        .map(|(dark, light)| {
            let alpha = 255 - (((light >> 16) & 255) as i32 - ((dark >> 16) & 255) as i32).clamp(0, 255) as u32;
            (alpha << 24) | (dark & 0x00FF_FFFF)
        })
        .collect();
    let mask = vec![0u8; (((size + 15) / 16) * 2 * size) as usize];
    unsafe {
        let color = CreateBitmap(size, size, 1, 32, pixels.as_ptr() as *const u8);
        let shape = CreateBitmap(size, size, 1, 1, mask.as_ptr());
        let icon = CreateIconIndirect(&IconInfo { icon: 1, x: 0, y: 0, mask: shape, color });
        DeleteObject(color);
        DeleteObject(shape);
        icon
    }
}

pub fn open(state: Json, ui: Json, vault: Vault, root: String) -> Result<()> {
    unsafe { SetProcessDpiAwarenessContext(-4); }
    picture::start();
    let class_name = wide("RLLM");
    let title = wide(&caption_for(&ui));
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let cursor = unsafe { LoadCursorW(0, 32512 as *const u16) };
    let class = WndClassEx {
        size: std::mem::size_of::<WndClassEx>() as u32,
        style: 0,
        wnd_proc: wndproc,
        cls_extra: 0,
        wnd_extra: 0,
        instance,
        icon: emblem(48),
        cursor,
        background: 0,
        menu: std::ptr::null(),
        class: class_name.as_ptr(),
        small_icon: emblem(20),
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        let code = unsafe { GetLastError() };
        return Err(format!("The desktop window could not be registered ({code})"));
    }
    let mut work = Rect { left: 0, top: 0, right: 1280, bottom: 800 };
    unsafe { SystemParametersInfoW(0x30, 0, &mut work, 0); }
    let screen_w = (work.right - work.left).max(960);
    let screen_h = (work.bottom - work.top).max(640);
    let window_w = (screen_w * 3 / 4).clamp(960, screen_w - 32);
    let window_h = (screen_h * 5 / 6).clamp(640, screen_h - 32);
    let frame = unsafe {
        CreateWindowExW(0, class_name.as_ptr(), title.as_ptr(), WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, work.left + (screen_w - window_w) / 2, work.top + (screen_h - window_h) / 3, window_w, window_h, 0, 0, instance, std::ptr::null_mut())
    };
    if frame == 0 {
        return Err("The desktop window could not be opened".into());
    }
    FRAME.store(frame, Ordering::Relaxed);
    unsafe {
        SendMessageW(frame, WM_SETICON, 1, emblem(48));
        SendMessageW(frame, WM_SETICON, 0, emblem(20));
    }
    let dpi = unsafe { GetDpiForWindow(frame) }.max(96);
    let mut area = Rect { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(frame, &mut area); }
    let mut app = App {
        state,
        ui,
        vault,
        root,
        frame,
        draft: 0,
        fields: Vec::new(),
        scroll: 0,
        stick: true,
        modal_scroll: 0,
        page_at: BTreeMap::new(),
        busy: false,
        tick: 0,
        note: String::new(),
        menu: false,
        synced_draft: String::new(),
        draft_lines: 1,
        dpi,
        width: area.right.max(1),
        height: area.bottom.max(1),
        scene: Scene::default(),
        pointer: (-1, -1),
        hover: None,
        pressed: None,
        press_at: None,
        tracking: false,
        fonts: [0; FONTS],
        lines: [16; FONTS],
        font_key: String::new(),
        marks: BTreeMap::new(),
        memo: BTreeMap::new(),
        canvas: unsafe { CreateCompatibleDC(0) },
        bitmap: 0,
        bits: std::ptr::null_mut(),
        size: (0, 0),
        backdrop: Vec::new(),
        backdrop_key: String::new(),
        walls: BTreeMap::new(),
        brush: 0,
        look_key: String::new(),
    };
    tune(&mut app);
    app.draft = make_edit(&app, true, false, "Ask anything");
    app.synced_draft = text(&app.ui, "draft");
    set_text(app.draft, &app.synced_draft);
    place(&mut app);
    app.draft_lines = draft_lines(&app);
    place(&mut app);
    let draft = app.draft;
    *APP.lock().map_err(|_| "Desktop session is unavailable".to_string())? = Some(app);
    unsafe {
        ShowWindow(frame, 5);
        SetFocus(draft);
    }
    let mut message = Msg { hwnd: 0, message: 0, wparam: 0, lparam: 0, time: 0, point_x: 0, point_y: 0 };
    loop {
        let code = unsafe { GetMessageW(&mut message, 0, 0, 0) };
        if code == 0 || code == -1 {
            break;
        }
        if key(&message) {
            continue;
        }
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

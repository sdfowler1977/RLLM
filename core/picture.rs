//! The bundled wallpapers. The files are the ones the Android build packages;
//! the Windows imaging component decodes them. A picture that cannot be
//! decoded is reported, and the window keeps its plain surface.
use super::raster::Picture;
use std::ffi::c_void;

static DARK: &[u8] = include_bytes!("../res/drawable-nodpi/wallpaper_dark.webp");
static LIGHT: &[u8] = include_bytes!("../platform/windows/wallpaper_light.jpg");

#[repr(C)]
struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}

const FACTORY_CLASS: Guid = Guid { a: 0xcacaf262, b: 0x9370, c: 0x4615, d: [0xa1, 0x3b, 0x9f, 0x55, 0x39, 0xda, 0x4c, 0x0a] };
const FACTORY: Guid = Guid { a: 0xec5ec8a9, b: 0xc395, c: 0x4314, d: [0x9c, 0x77, 0x54, 0xd7, 0xa9, 0x35, 0xff, 0x70] };
const BGRA: Guid = Guid { a: 0x6fddc324, b: 0x4e03, c: 0x4bfe, d: [0xb1, 0x85, 0x3d, 0x77, 0x76, 0x8d, 0xc9, 0x0f] };

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, model: u32) -> i32;
    fn CoCreateInstance(class: *const Guid, outer: *mut c_void, context: u32, interface: *const Guid, out: *mut *mut c_void) -> i32;
}

unsafe fn slot(object: *mut c_void, index: usize) -> usize {
    let table = *(object as *const *const usize);
    *table.add(index)
}

struct Held(*mut c_void);

impl Drop for Held {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(slot(self.0, 2));
                release(self.0);
            }
        }
    }
}

fn failed(step: &str, code: i32) -> String {
    format!("{step} failed ({:#010x})", code as u32)
}

pub fn start() {
    unsafe { CoInitializeEx(std::ptr::null_mut(), 2); }
}

fn factory() -> Result<Held, String> {
    let mut out = std::ptr::null_mut();
    let code = unsafe { CoCreateInstance(&FACTORY_CLASS, std::ptr::null_mut(), 1, &FACTORY, &mut out) };
    if code < 0 || out.is_null() {
        return Err(failed("The imaging component", code));
    }
    Ok(Held(out))
}

fn pixels(factory: &Held, decoder: &Held) -> Result<Picture, String> {
    unsafe {
        let mut frame = std::ptr::null_mut();
        let get_frame: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32 = std::mem::transmute(slot(decoder.0, 13));
        let code = get_frame(decoder.0, 0, &mut frame);
        if code < 0 || frame.is_null() {
            return Err(failed("Picture frame", code));
        }
        let frame = Held(frame);
        let mut converter = std::ptr::null_mut();
        let create: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(slot(factory.0, 10));
        let code = create(factory.0, &mut converter);
        if code < 0 || converter.is_null() {
            return Err(failed("Picture converter", code));
        }
        let converter = Held(converter);
        let initialize: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, u32, *mut c_void, f64, u32) -> i32 = std::mem::transmute(slot(converter.0, 8));
        let code = initialize(converter.0, frame.0, &BGRA, 0, std::ptr::null_mut(), 0.0, 0);
        if code < 0 {
            return Err(failed("Picture conversion", code));
        }
        let (mut width, mut height) = (0u32, 0u32);
        let size: unsafe extern "system" fn(*mut c_void, *mut u32, *mut u32) -> i32 = std::mem::transmute(slot(converter.0, 3));
        let code = size(converter.0, &mut width, &mut height);
        if code < 0 || width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(failed("Picture size", code));
        }
        let mut pixels = vec![0u32; width as usize * height as usize];
        let copy: unsafe extern "system" fn(*mut c_void, *const c_void, u32, u32, *mut u8) -> i32 = std::mem::transmute(slot(converter.0, 7));
        let code = copy(converter.0, std::ptr::null(), width * 4, width * height * 4, pixels.as_mut_ptr() as *mut u8);
        if code < 0 {
            return Err(failed("Picture pixels", code));
        }
        Ok(Picture { width: width as usize, height: height as usize, pixels })
    }
}

fn decode(bytes: &[u8]) -> Result<Picture, String> {
    let factory = factory()?;
    unsafe {
        let mut stream = std::ptr::null_mut();
        let create: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(slot(factory.0, 14));
        let code = create(factory.0, &mut stream);
        if code < 0 || stream.is_null() {
            return Err(failed("Picture stream", code));
        }
        let stream = Held(stream);
        let fill: unsafe extern "system" fn(*mut c_void, *const u8, u32) -> i32 = std::mem::transmute(slot(stream.0, 16));
        let code = fill(stream.0, bytes.as_ptr(), bytes.len() as u32);
        if code < 0 {
            return Err(failed("Picture bytes", code));
        }
        let mut decoder = std::ptr::null_mut();
        let open: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, u32, *mut *mut c_void) -> i32 = std::mem::transmute(slot(factory.0, 4));
        let code = open(factory.0, stream.0, std::ptr::null(), 0, &mut decoder);
        if code < 0 || decoder.is_null() {
            return Err(failed("Picture decoder", code));
        }
        pixels(&factory, &Held(decoder))
    }
}

/// A picture the user chose, read from its file.
pub fn from_file(path: &str) -> Result<Picture, String> {
    use std::os::windows::ffi::OsStrExt;
    let factory = factory()?;
    let name: Vec<u16> = std::ffi::OsStr::new(path).encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let mut decoder = std::ptr::null_mut();
        let open: unsafe extern "system" fn(*mut c_void, *const u16, *const Guid, u32, u32, *mut *mut c_void) -> i32 = std::mem::transmute(slot(factory.0, 3));
        let code = open(factory.0, name.as_ptr(), std::ptr::null(), 0x8000_0000, 0, &mut decoder);
        if code < 0 || decoder.is_null() {
            return Err(failed("Picture file", code));
        }
        pixels(&factory, &Held(decoder)).map(fit)
    }
}

fn fit(mut picture: Picture) -> Picture {
    while picture.width > 2560 || picture.height > 2560 {
        picture = picture.half();
    }
    picture
}

/// The bundled wallpaper for the theme.
pub fn bundled(dark: bool) -> Result<Picture, String> {
    decode(if dark { DARK } else { LIGHT }).map(fit)
}

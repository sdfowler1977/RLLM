//! General connected-device observation. Not tied to any one kind of hardware:
//! it enumerates what is attached to this host and writes a report file so
//! inspect/read/reconstruct and the recursive inquiry reason over it like any
//! other observation. On Windows it reads the live PnP device tree (which
//! includes USB); on other hosts it walks the system device tree. Enumeration is
//! read-only; it reports what it can see and names what it cannot, rather than
//! refusing.
use super::*;

pub(super) fn validate(a: &Json) -> Result<()> {
    let Obj(f) = a else { return Err("Device observation requires an object".into()); };
    if f.keys().any(|k| !["mode", "output", "note"].contains(&k.as_str())) {
        return Err("Device observation accepts mode, output and note".into());
    }
    if text(a, "mode") != "list" {
        return Err("Choose mode list to enumerate connected devices".into());
    }
    let output = get(a, "output").text()?;
    if output.is_empty() || output.len() > 255 || output == "." || output == ".." || output.contains(['/', '\\', '\0']) {
        return Err("Choose a new workspace-root output filename for the report".into());
    }
    if text(a, "note").len() > 512 { return Err("note exceeds 512 bytes".into()); }
    Ok(())
}

pub(super) fn run(root: &Path, args: &Json, access: &operation::InspectionAccess) -> Result<String> {
    validate(args)?;
    let cancelled = || access.cancelled();
    let report = enumerate(&cancelled);
    let name = text(args, "output");
    let offset = capture_file::save_bytes(root, &name, report.as_bytes(), &cancelled)?;
    Ok(format!("Wrote {offset} bytes of device observations to {name}. Read or inspect {name} for the enumerated devices."))
}

#[cfg(windows)]
fn enumerate(cancelled: &impl Fn() -> bool) -> String {
    let mut out = String::from("Device observation: connected devices (Windows PnP tree)\n\n");
    let devices = win::present_devices(cancelled);
    out.push_str(&format!("Devices present: {}\n\n", devices.len()));
    for (i, d) in devices.iter().enumerate() {
        out.push_str(&format!("[{}] {}\n", i + 1, d.name));
        if !d.manufacturer.is_empty() { out.push_str(&format!("    manufacturer: {}\n", d.manufacturer)); }
        if !d.hardware_id.is_empty() { out.push_str(&format!("    hardware id:  {}\n", d.hardware_id)); }
    }
    out
}

#[cfg(not(windows))]
fn enumerate(cancelled: &impl Fn() -> bool) -> String {
    let mut out = String::from("Device observation: connected devices (system device tree)\n\n");
    let mut count = 0usize;
    // USB devices as exposed by the kernel.
    if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
        out.push_str("USB devices (/sys/bus/usb/devices):\n");
        for entry in entries.flatten() {
            if cancelled() { break; }
            let p = entry.path();
            let read = |name: &str| fs::read_to_string(p.join(name)).map(|s| s.trim().to_string()).unwrap_or_default();
            let vid = read("idVendor");
            let pid = read("idProduct");
            if vid.is_empty() && pid.is_empty() { continue; }
            count += 1;
            let product = read("product");
            let manufacturer = read("manufacturer");
            out.push_str(&format!("  {} {}:{} {} {}\n", entry.file_name().to_string_lossy(), vid, pid, manufacturer, product));
        }
        out.push('\n');
    } else {
        out.push_str("USB device tree (/sys/bus/usb/devices) is not readable on this host.\n\n");
    }
    // Character devices commonly used for serial/attached hardware.
    if let Ok(entries) = fs::read_dir("/dev") {
        let mut serial = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("ttyUSB") || name.starts_with("ttyACM") || name.starts_with("ttyS") { serial.push(name); }
        }
        if !serial.is_empty() { serial.sort(); out.push_str(&format!("Serial devices (/dev): {}\n", serial.join(", "))); }
    }
    out.push_str(&format!("\nUSB devices listed: {count}\n"));
    out
}

#[cfg(windows)]
mod win {
    pub struct Device { pub name: String, pub manufacturer: String, pub hardware_id: String }

    #[repr(C)]
    struct Guid { d1: u32, d2: u16, d3: u16, d4: [u8; 8] }
    #[repr(C)]
    struct DevInfoData { cb_size: u32, class_guid: Guid, dev_inst: u32, reserved: usize }

    #[link(name = "setupapi")]
    extern "system" {
        fn SetupDiGetClassDevsW(class: *const Guid, enumerator: *const u16, parent: isize, flags: u32) -> isize;
        fn SetupDiEnumDeviceInfo(set: isize, index: u32, data: *mut DevInfoData) -> i32;
        fn SetupDiGetDeviceRegistryPropertyW(set: isize, data: *mut DevInfoData, property: u32, reg_type: *mut u32, buf: *mut u8, size: u32, required: *mut u32) -> i32;
        fn SetupDiDestroyDeviceInfoList(set: isize) -> i32;
    }

    const DIGCF_PRESENT: u32 = 0x2;
    const DIGCF_ALLCLASSES: u32 = 0x4;
    const SPDRP_DEVICEDESC: u32 = 0x0;
    const SPDRP_HARDWAREID: u32 = 0x1;
    const SPDRP_FRIENDLYNAME: u32 = 0xC;
    const SPDRP_MFG: u32 = 0xB;
    const INVALID_HANDLE: isize = -1;

    fn property(set: isize, data: *mut DevInfoData, id: u32, multi: bool) -> String {
        let mut buf = vec![0u8; 2048];
        let mut required = 0u32;
        let ok = unsafe { SetupDiGetDeviceRegistryPropertyW(set, data, id, std::ptr::null_mut(), buf.as_mut_ptr(), buf.len() as u32, &mut required) };
        if ok == 0 { return String::new(); }
        let len = (required as usize / 2).min(buf.len() / 2);
        let units: Vec<u16> = (0..len).map(|i| u16::from_le_bytes([buf[i * 2], buf[i * 2 + 1]])).collect();
        if multi {
            // REG_MULTI_SZ: null-separated list, double-null terminated.
            units.split(|u| *u == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf16_lossy(s)).collect::<Vec<_>>().join("; ")
        } else {
            let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
            String::from_utf16_lossy(&units[..end])
        }
    }

    pub fn present_devices(cancelled: &impl Fn() -> bool) -> Vec<Device> {
        let mut devices = Vec::new();
        let set = unsafe { SetupDiGetClassDevsW(std::ptr::null(), std::ptr::null(), 0, DIGCF_PRESENT | DIGCF_ALLCLASSES) };
        if set == INVALID_HANDLE || set == 0 { return devices; }
        let mut index = 0u32;
        loop {
            if cancelled() || index > 65535 { break; }
            let mut data = DevInfoData { cb_size: std::mem::size_of::<DevInfoData>() as u32, class_guid: Guid { d1: 0, d2: 0, d3: 0, d4: [0; 8] }, dev_inst: 0, reserved: 0 };
            if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) } == 0 { break; }
            index += 1;
            let friendly = property(set, &mut data, SPDRP_FRIENDLYNAME, false);
            let name = if friendly.is_empty() { property(set, &mut data, SPDRP_DEVICEDESC, false) } else { friendly };
            if name.is_empty() { continue; }
            devices.push(Device {
                name,
                manufacturer: property(set, &mut data, SPDRP_MFG, false),
                hardware_id: property(set, &mut data, SPDRP_HARDWAREID, true),
            });
        }
        unsafe { SetupDiDestroyDeviceInfoList(set) };
        devices
    }
}

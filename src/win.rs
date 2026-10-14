//! Windows specifics: window chrome (rounded corners, resize edges, always-on-top), monitor work areas
//! and the system preferences Reyer adapts to (transparency effects, high contrast, animations,
//! light theme, battery saver).
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetCurrentProcessExplicitAppUserModelID, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowRect, IsZoomed, MessageBoxW, SetWindowPos, SystemParametersInfoW, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP,
    HTTOPLEFT, HTTOPRIGHT, HWND_NOTOPMOST, HWND_TOPMOST, MB_ICONINFORMATION, SPI_GETCLIENTAREAANIMATION, SPI_GETHIGHCONTRAST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, WM_NCHITTEST,
};

/// Logical px inside the window edge that resize.
const RESIZE_BORDER: i32 = 6;
const HCF_HIGHCONTRASTON: u32 = 1;
static FULLSCREEN: AtomicBool = AtomicBool::new(false);

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn set_fullscreen_flag(on: bool) {
    FULLSCREEN.store(on, Ordering::Relaxed);
}

pub fn set_rounded(hwnd: isize, rounded: bool) {
    let pref: i32 = if rounded { 2 } else { 1 }; // DWMWCP_ROUND / DWMWCP_DONOTROUND
    unsafe {
        DwmSetWindowAttribute(hwnd as HWND, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &pref as *const i32 as *const c_void, 4);
    }
}

/// Native z-order change: the window (and its GL context) is never recreated.
pub fn set_topmost(hwnd: isize, on: bool) {
    unsafe {
        SetWindowPos(hwnd as HWND, if on { HWND_TOPMOST } else { HWND_NOTOPMOST }, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub fn set_app_id() {
    unsafe { SetCurrentProcessExplicitAppUserModelID(wide("Reyer.Player").as_ptr()) };
}

pub fn message_box(text: &str, title: &str) {
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide(title).as_ptr(), MB_ICONINFORMATION) };
}

unsafe extern "system" fn hit_test_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, _data: usize) -> LRESULT {
    if msg == WM_NCHITTEST && IsZoomed(hwnd) == 0 && !FULLSCREEN.load(Ordering::Relaxed) {
        let x = (lparam & 0xFFFF) as u16 as i16 as i32;
        let y = ((lparam >> 16) & 0xFFFF) as u16 as i16 as i32;
        let mut r: RECT = std::mem::zeroed();
        GetWindowRect(hwnd, &mut r);
        let b = (RESIZE_BORDER as f32 * GetDpiForWindow(hwnd).max(96) as f32 / 96.0).round() as i32;
        let (left, right, top, bottom) = (x < r.left + b, x >= r.right - b, y < r.top + b, y >= r.bottom - b);
        let code = match (left, right, top, bottom) {
            (true, _, true, _) => HTTOPLEFT,
            (_, true, true, _) => HTTOPRIGHT,
            (true, _, _, true) => HTBOTTOMLEFT,
            (_, true, _, true) => HTBOTTOMRIGHT,
            (true, ..) => HTLEFT,
            (_, true, ..) => HTRIGHT,
            (_, _, true, _) => HTTOP,
            (_, _, _, true) => HTBOTTOM,
            _ => 0,
        };
        if code != 0 {
            return code as LRESULT;
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Resize edges for the frameless window; moving is done by dragging the video area.
pub fn install_resize_edges(hwnd: isize) {
    unsafe { SetWindowSubclass(hwnd as HWND, Some(hit_test_proc), 1, 0) };
}

/// Work areas of all monitors (without the taskbar) and the primary one, in physical pixels.
pub fn work_areas() -> (Vec<[i32; 4]>, [i32; 4]) {
    unsafe extern "system" fn each(mon: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> i32 {
        let list = &mut *(data as *mut Vec<([i32; 4], bool)>);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(mon, &mut info) != 0 {
            let w = info.rcWork;
            list.push(([w.left, w.top, w.right - w.left, w.bottom - w.top], info.dwFlags & 1 != 0));
        }
        1
    }
    let mut list: Vec<([i32; 4], bool)> = Vec::new();
    unsafe { EnumDisplayMonitors(std::ptr::null_mut(), std::ptr::null(), Some(each), &mut list as *mut _ as LPARAM) };
    let primary = list.iter().find(|(_, p)| *p).or(list.first()).map(|(r, _)| *r).unwrap_or([0, 0, 1920, 1040]);
    (list.into_iter().map(|(r, _)| r).collect(), primary)
}

// ---- system preferences ----------------------------------------------------------------------------
fn personalize(name: &str, default: u32) -> u32 {
    let mut value: u32 = default;
    let mut size: u32 = 4;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").as_ptr(),
            wide(name).as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    if rc == 0 {
        value
    } else {
        default
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SystemPrefs {
    pub reduce_transparency: bool,
    pub increase_contrast: bool,
    pub reduce_motion: bool,
    pub light_theme: bool,
    pub battery_saver: bool,
}

#[repr(C)]
struct HighContrastW {
    cb_size: u32,
    flags: u32,
    default_scheme: *mut u16,
}

impl SystemPrefs {
    pub fn read() -> SystemPrefs {
        unsafe {
            let mut hc = HighContrastW { cb_size: std::mem::size_of::<HighContrastW>() as u32, flags: 0, default_scheme: std::ptr::null_mut() };
            let ok = SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cb_size, &mut hc as *mut _ as *mut c_void, 0) != 0;
            let mut animations: i32 = 1;
            SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut animations as *mut i32 as *mut c_void, 0);
            let mut power: SYSTEM_POWER_STATUS = std::mem::zeroed();
            let saver = GetSystemPowerStatus(&mut power) != 0 && power.SystemStatusFlag & 1 != 0;
            SystemPrefs {
                reduce_transparency: personalize("EnableTransparency", 1) == 0,
                increase_contrast: ok && hc.flags & HCF_HIGHCONTRASTON != 0,
                reduce_motion: animations == 0,
                light_theme: personalize("AppsUseLightTheme", 0) != 0,
                battery_saver: saver,
            }
        }
    }
}

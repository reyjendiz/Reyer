//! Quick Look for Explorer: hold Space on a video or audio file (in a folder window or on the desktop)
//! and Reyer previews it until Space is released, like on a Mac.
//!
//! A small background process (`--quicklook-agent`, started with Windows) watches the keyboard with a
//! low-level hook. Explorer's selection is read through Shell.Application by a PowerShell helper that
//! stays running, so a lookup takes a few milliseconds.
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
use windows_sys::Win32::System::Threading::{CreateMutexW, OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_SPACE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetMessageW, GetWindowThreadProcessId, KillTimer, SetTimer,
    SetWindowsHookExW, GUITHREADINFO, HC_ACTION, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER,
};

const MUTEX: &str = "Local\\ReyerQuickLook";
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "Reyer Quick Look";
/// How long Space must be held before the preview opens; a normal key press never triggers it.
const HOLD_MS: u32 = 350;

/// Called by the player at startup: registers the agent with Windows and starts it, or removes it.
pub fn ensure(enabled: bool) {
    let w = crate::win::wide;
    let Ok(exe) = std::env::current_exe() else { return };
    unsafe {
        if enabled {
            let cmd = w(&format!("\"{}\" --quicklook-agent", exe.display()));
            RegSetKeyValueW(HKEY_CURRENT_USER, w(RUN_KEY).as_ptr(), w(RUN_VALUE).as_ptr(), REG_SZ, cmd.as_ptr().cast(), (cmd.len() * 2) as u32);
            if OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, w(MUTEX).as_ptr()).is_null() {
                let _ = Command::new(&exe).arg("--quicklook-agent").creation_flags(0x0800_0000).spawn(); // CREATE_NO_WINDOW
            }
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, w(RUN_KEY).as_ptr(), w(RUN_VALUE).as_ptr());
        }
    }
}

struct Agent {
    space_down: bool,
    timer: usize,
    target: HWND,
    preview: Option<Child>,
    helper: Option<(Child, ChildStdin, BufReader<ChildStdout>)>,
}

thread_local! {
    static AGENT: RefCell<Agent> = RefCell::new(Agent { space_down: false, timer: 0, target: std::ptr::null_mut(), preview: None, helper: None });
}

pub fn run_agent() {
    unsafe {
        // one agent per session; the handle lives as long as the process
        CreateMutexW(std::ptr::null(), 1, crate::win::wide(MUTEX).as_ptr());
        if windows_sys::Win32::Foundation::GetLastError() == windows_sys::Win32::Foundation::ERROR_ALREADY_EXISTS {
            return;
        }
        // warm the PowerShell helper up now: its first answer takes too long for a keyboard hook
        AGENT.with(|a| {
            let _ = selection(&mut a.borrow_mut(), std::ptr::null_mut());
        });
        if SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), std::ptr::null_mut(), 0).is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_TIMER {
                on_hold();
            }
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = &*(lparam as *const KBDLLHOOKSTRUCT);
        if k.vkCode == VK_SPACE as u32 {
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(wparam as u32, WM_KEYUP | WM_SYSKEYUP);
            let swallow = AGENT.with(|a| {
                let mut a = a.borrow_mut();
                if down && !a.space_down {
                    a.space_down = true;
                    if let Some(t) = explorer_target() {
                        a.target = t;
                        a.timer = SetTimer(std::ptr::null_mut(), 0, HOLD_MS, None);
                    }
                    false
                } else if up {
                    a.space_down = false;
                    if a.timer != 0 {
                        KillTimer(std::ptr::null_mut(), a.timer);
                        a.timer = 0;
                    }
                    match a.preview.take() {
                        Some(mut p) => {
                            let _ = p.kill();
                            true
                        }
                        None => false,
                    }
                } else {
                    // auto-repeat while the preview is up would reach the window under it
                    a.preview.is_some()
                }
            });
            if swallow {
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The Explorer window (or the desktop) in front, unless a text box in it has the focus or a modifier is held.
fn explorer_target() -> Option<HWND> {
    unsafe {
        if [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN].iter().any(|&v| GetAsyncKeyState(v as i32) < 0) {
            return None;
        }
        let fg = GetForegroundWindow();
        if !matches!(class_of(fg).as_str(), "CabinetWClass" | "Progman" | "WorkerW") {
            return None;
        }
        let mut gti: GUITHREADINFO = std::mem::zeroed();
        gti.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
        let thread = GetWindowThreadProcessId(fg, std::ptr::null_mut());
        if GetGUIThreadInfo(thread, &mut gti) != 0 && !gti.hwndFocus.is_null() && class_of(gti.hwndFocus) == "Edit" {
            return None; // renaming a file or typing in a field
        }
        Some(fg)
    }
}

fn on_hold() {
    AGENT.with(|a| {
        let mut a = a.borrow_mut();
        if a.timer != 0 {
            unsafe { KillTimer(std::ptr::null_mut(), a.timer) };
            a.timer = 0;
        }
        if !a.space_down || a.preview.is_some() || !crate::settings::Settings::load(&crate::settings::data_dir().join("settings.json")).quick_look {
            return;
        }
        let target = a.target;
        let Some(path) = selection(&mut a, target) else { return };
        if !crate::media::is_media(std::path::Path::new(&path)) {
            return;
        }
        let r = preview_rect(target);
        let Ok(exe) = std::env::current_exe() else { return };
        a.preview = Command::new(exe).args(["--preview", "--rect", &format!("{},{},{},{}", r[0], r[1], r[2], r[3]), &path]).spawn().ok();
    });
}

/// 62% of the work area of the monitor the Explorer window is on, centered.
fn preview_rect(hwnd: HWND) -> [i32; 4] {
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi);
        let RECT { left, top, right, bottom } = mi.rcWork;
        let (w, h) = (((right - left) as f32 * 0.62) as i32, ((bottom - top) as f32 * 0.62) as i32);
        [left + (right - left - w) / 2, top + (bottom - top - h) / 2, w, h]
    }
}

const HELPER: &str = "[Console]::OutputEncoding=[Text.Encoding]::UTF8;$sa=New-Object -ComObject Shell.Application;\
while($null -ne ($l=[Console]::In.ReadLine())){$p='';try{$w=$null;\
if($l -eq 'desktop'){$w=$sa.Windows().Item()}else{foreach($x in $sa.Windows()){if([int64]$x.HWND -eq [int64]$l){$w=$x;break}}};\
if($w){$s=@($w.Document.SelectedItems());if($s.Count -eq 1){$p=$s[0].Path}}}catch{};[Console]::Out.WriteLine($p);[Console]::Out.Flush()}";

/// The one selected item in that window, if exactly one is selected.
// shortcut: with several tabs in one Explorer window the first tab is asked; upgrade if tabs matter
fn selection(a: &mut Agent, hwnd: HWND) -> Option<String> {
    let key = if class_of(hwnd) == "CabinetWClass" { (hwnd as isize).to_string() } else { "desktop".into() };
    for _ in 0..2 {
        if a.helper.is_none() {
            let mut child = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", HELPER])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .creation_flags(0x0800_0000)
                .spawn()
                .ok()?;
            let (stdin, stdout) = (child.stdin.take()?, child.stdout.take()?);
            a.helper = Some((child, stdin, BufReader::new(stdout)));
        }
        let (_, stdin, stdout) = a.helper.as_mut()?;
        let mut line = String::new();
        if writeln!(stdin, "{key}").is_ok() && stdin.flush().is_ok() && stdout.read_line(&mut line).unwrap_or(0) > 0 {
            let p = line.trim().to_string();
            return (!p.is_empty()).then_some(p);
        }
        a.helper = None; // the helper died; start a new one and ask again
    }
    None
}

use std::mem::size_of;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetLastActivePopup, GetWindowLongW, GetWindowTextLengthW,
    GetWindowTextW, IsWindowVisible, GA_ROOTOWNER, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
};

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: HWND,
    pub title: String,
}

unsafe impl Send for WindowInfo {}
unsafe impl Sync for WindowInfo {}

static CACHE: parking_lot::Mutex<Option<(Instant, Vec<WindowInfo>)>> = parking_lot::Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_millis(250);

pub fn invalidate_window_cache() {
    *CACHE.lock() = None;
}

pub fn is_cloaked(hwnd: HWND) -> bool {
    unsafe {
        let mut cloaked: u32 = 0;
        let res = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            size_of::<u32>() as u32,
        );
        if res.is_ok() {
            cloaked != 0
        } else {
            false
        }
    }
}

pub fn is_alt_tab_window(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }

        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return false;
        }

        let mut buf = vec![0u16; (len + 1) as usize];
        let read = GetWindowTextW(hwnd, &mut buf);
        if read <= 0 {
            return false;
        }
        let title = String::from_utf16_lossy(&buf[..read as usize]);

        if title == "Program Manager" || title == "Windows Input Experience" {
            return false;
        }

        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if (ex_style & WS_EX_TOOLWINDOW.0) != 0 {
            return false;
        }

        let root = GetAncestor(hwnd, GA_ROOTOWNER);
        if GetLastActivePopup(root) != hwnd {
            return false;
        }

        if is_cloaked(hwnd) {
            return false;
        }

        true
    }
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<WindowInfo>);
    if is_alt_tab_window(hwnd) {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let read = GetWindowTextW(hwnd, &mut buf);
            if read > 0 {
                let title = String::from_utf16_lossy(&buf[..read as usize]);
                list.push(WindowInfo { hwnd, title });
            }
        }
    }
    BOOL(1)
}

pub fn get_open_windows(blocklist: &[String], force_refresh: bool) -> Vec<WindowInfo> {
    let now = Instant::now();
    let mut cache = CACHE.lock();

    if !force_refresh {
        if let Some((ts, ref data)) = *cache {
            if now.duration_since(ts) < CACHE_TTL {
                return data
                    .iter()
                    .filter(|w| !blocklist.contains(&w.title))
                    .cloned()
                    .collect();
            }
        }
    }

    let mut windows = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut windows as *mut _ as isize),
        );
    }

    *cache = Some((now, windows.clone()));

    windows
        .into_iter()
        .filter(|w| !blocklist.contains(&w.title))
        .collect()
}

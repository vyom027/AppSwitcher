use crate::window::enumerator::{is_alt_tab_window, WindowInfo};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

static MRU_LIST: RwLock<VecDeque<isize>> = RwLock::new(VecDeque::new());
static WATCHER_STARTED: AtomicBool = AtomicBool::new(false);
const MAX_MRU_CAPACITY: usize = 64;

pub fn note_focused(hwnd: HWND) {
    let raw = hwnd.0 as isize;
    if raw == 0 {
        return;
    }

    let mut list = MRU_LIST.write();
    if let Some(pos) = list.iter().position(|&h| h == raw) {
        list.remove(pos);
    }
    list.push_front(raw);

    if list.len() > MAX_MRU_CAPACITY {
        list.truncate(MAX_MRU_CAPACITY);
    }
}

pub fn mru_rank(hwnd: HWND) -> usize {
    let raw = hwnd.0 as isize;
    let list = MRU_LIST.read();
    list.iter().position(|&h| h == raw).unwrap_or(usize::MAX)
}

pub fn sort_by_mru(mut windows: Vec<WindowInfo>) -> Vec<WindowInfo> {
    windows.sort_by_key(|w| mru_rank(w.hwnd));
    windows
}

pub fn start_mru_watcher() {
    if WATCHER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    thread::spawn(|| {
        let mut last_hwnd = HWND(std::ptr::null_mut());
        loop {
            unsafe {
                let fg = GetForegroundWindow();
                if !fg.0.is_null() && fg != last_hwnd && is_alt_tab_window(fg) {
                    note_focused(fg);
                    last_hwnd = fg;
                }
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}


#![allow(dead_code)]

use crate::event::{AppEvent, WM_APP_EVENT};
use crossbeam_channel::Sender;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, GetAsyncKeyState, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_LCONTROL,
    VK_LMENU, VK_MENU, VK_RCONTROL, VK_RMENU, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetSystemMetrics, GetWindowLongW, GetWindowRect, IsZoomed,
    PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, GWL_STYLE, HHOOK, KBDLLHOOKSTRUCT,
    SM_CXSCREEN, SM_CYSCREEN, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WS_CAPTION,
};

static HOOK_HANDLE: parking_lot::Mutex<Option<usize>> = parking_lot::Mutex::new(None);
static ENABLED: AtomicBool = AtomicBool::new(true);
static MOD_DOWN: AtomicBool = AtomicBool::new(false);
static SEARCH_MODE: AtomicBool = AtomicBool::new(false);
static PICKER_MODE: AtomicBool = AtomicBool::new(false);
static PICKER_KEYS: parking_lot::Mutex<VecDeque<u32>> = parking_lot::Mutex::new(VecDeque::new());
static SEARCH_INPUT: parking_lot::Mutex<VecDeque<SearchInput>> =
    parking_lot::Mutex::new(VecDeque::new());
static ALTTAB_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRIGGER_VK: AtomicU32 = AtomicU32::new(VK_TAB.0 as u32);
static USE_CTRL: AtomicBool = AtomicBool::new(false);
static UI_HWND: AtomicUsize = AtomicUsize::new(0);

static EVENT_SENDER: parking_lot::RwLock<Option<Sender<AppEvent>>> = parking_lot::RwLock::new(None);

static LAST_FG_HWND: AtomicU32 = AtomicU32::new(0);
static LAST_FG_FS: AtomicBool = AtomicBool::new(false);
static LAST_FG_TIME: parking_lot::Mutex<Option<Instant>> = parking_lot::Mutex::new(None);

#[derive(Debug, Clone, Copy)]
pub enum SearchInput {
    Character(char),
    Backspace,
    Clear,
}

pub fn set_search_mode(enabled: bool) {
    SEARCH_MODE.store(enabled, Ordering::SeqCst);
    if !enabled {
        SEARCH_INPUT.lock().clear();
    }
}

/// While the picker is open, navigation keys are swallowed (so they don't
/// also reach the app underneath — e.g. Alt+Left would trigger browser Back)
/// and queued for the picker loop instead.
pub fn set_picker_mode(enabled: bool) {
    PICKER_MODE.store(enabled, Ordering::SeqCst);
    PICKER_KEYS.lock().clear();
}

pub fn take_picker_keys() -> Vec<u32> {
    PICKER_KEYS.lock().drain(..).collect()
}

/// True when the low-level hook is installed and will deliver picker keys.
pub fn is_installed() -> bool {
    HOOK_HANDLE.lock().is_some()
}

fn is_picker_key(vk: u32) -> bool {
    matches!(
        vk,
        0x0D // Enter
        | 0x1B // Esc
        | 0x23..=0x28 // End, Home, Left, Up, Right, Down
        | 0x2E // Delete
        | 0x31..=0x39 // 1-9
        | 0x51 // Q
        | 0x57 // W
    )
}

pub fn take_search_input() -> Vec<SearchInput> {
    SEARCH_INPUT.lock().drain(..).collect()
}

fn decode_search_key(vk: u32) -> Option<SearchInput> {
    if vk == 0x08 {
        return Some(SearchInput::Backspace);
    }
    if vk == 0x20 {
        return Some(SearchInput::Character(' '));
    }
    if vk == 0x41 && unsafe { (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 } {
        return Some(SearchInput::Clear);
    }
    let shifted = unsafe { (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0 };
    if (0x41..=0x5a).contains(&vk) {
        let ch = char::from_u32(if shifted { vk } else { vk + 0x20 })?;
        return Some(SearchInput::Character(ch));
    }
    if (0x30..=0x39).contains(&vk) {
        return char::from_u32(vk).map(SearchInput::Character);
    }
    let ch = match vk {
        0xba => {
            if shifted {
                ':'
            } else {
                ';'
            }
        }
        0xbb => {
            if shifted {
                '+'
            } else {
                '='
            }
        }
        0xbc => {
            if shifted {
                '<'
            } else {
                ','
            }
        }
        0xbd => {
            if shifted {
                '_'
            } else {
                '-'
            }
        }
        0xbe => {
            if shifted {
                '>'
            } else {
                '.'
            }
        }
        0xbf => {
            if shifted {
                '?'
            } else {
                '/'
            }
        }
        0xc0 => {
            if shifted {
                '~'
            } else {
                '`'
            }
        }
        0xdb => {
            if shifted {
                '{'
            } else {
                '['
            }
        }
        0xdc => {
            if shifted {
                '|'
            } else {
                '\\'
            }
        }
        0xdd => {
            if shifted {
                '}'
            } else {
                ']'
            }
        }
        0xde => {
            if shifted {
                '"'
            } else {
                '\''
            }
        }
        _ => return None,
    };
    Some(SearchInput::Character(ch))
}

fn is_search_key(vk: u32) -> bool {
    (0x30..=0x5a).contains(&vk)
        || (0xba..=0xc0).contains(&vk)
        || (0xdb..=0xdf).contains(&vk)
        || vk == 0x08
        || vk == 0x20
}

pub fn configure(mod_name: &str, trigger_vk: u32) {
    let is_ctrl = mod_name.eq_ignore_ascii_case("ctrl");
    USE_CTRL.store(is_ctrl, Ordering::SeqCst);
    TRIGGER_VK.store(trigger_vk, Ordering::SeqCst);
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::SeqCst);
}

fn foreground_is_fullscreen() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }

        let hwnd_u32 = hwnd.0 as usize as u32;
        let now = Instant::now();

        {
            let last_time = LAST_FG_TIME.lock();
            if let Some(t) = *last_time {
                if now.duration_since(t) < Duration::from_millis(350)
                    && LAST_FG_HWND.load(Ordering::Relaxed) == hwnd_u32
                {
                    return LAST_FG_FS.load(Ordering::Relaxed);
                }
            }
        }

        if IsZoomed(hwnd).as_bool() {
            LAST_FG_HWND.store(hwnd_u32, Ordering::Relaxed);
            LAST_FG_FS.store(false, Ordering::Relaxed);
            *LAST_FG_TIME.lock() = Some(now);
            return false;
        }

        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        if (style & WS_CAPTION.0) != 0 {
            LAST_FG_HWND.store(hwnd_u32, Ordering::Relaxed);
            LAST_FG_FS.store(false, Ordering::Relaxed);
            *LAST_FG_TIME.lock() = Some(now);
            return false;
        }

        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        // Compare against the monitor the window is on, not the primary one.
        let mon = crate::window::capture::window_monitor_rect(hwnd).unwrap_or(RECT {
            left: 0,
            top: 0,
            right: GetSystemMetrics(SM_CXSCREEN),
            bottom: GetSystemMetrics(SM_CYSCREEN),
        });

        let is_fs = rect.left <= mon.left
            && rect.top <= mon.top
            && rect.right >= mon.right
            && rect.bottom >= mon.bottom;

        LAST_FG_HWND.store(hwnd_u32, Ordering::Relaxed);
        LAST_FG_FS.store(is_fs, Ordering::Relaxed);
        *LAST_FG_TIME.lock() = Some(now);
        is_fs
    }
}

unsafe extern "system" fn hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 {
        let enabled = ENABLED.load(Ordering::Relaxed) && !crate::input::is_paused();
        let kb = &*(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode;
        let is_down = w_param.0 == WM_KEYDOWN as usize || w_param.0 == WM_SYSKEYDOWN as usize;
        let is_up = w_param.0 == WM_KEYUP as usize || w_param.0 == WM_SYSKEYUP as usize;

        let use_ctrl = USE_CTRL.load(Ordering::Relaxed);
        let is_mod_key = if use_ctrl {
            vk == VK_CONTROL.0 as u32 || vk == VK_LCONTROL.0 as u32 || vk == VK_RCONTROL.0 as u32
        } else {
            vk == VK_MENU.0 as u32 || vk == VK_LMENU.0 as u32 || vk == VK_RMENU.0 as u32
        };

        if is_mod_key {
            if is_down {
                MOD_DOWN.store(true, Ordering::SeqCst);
            } else if is_up {
                MOD_DOWN.store(false, Ordering::SeqCst);
                // Modifier released -> commit Alt+Tab if active
                if ALTTAB_ACTIVE.swap(false, Ordering::SeqCst) {
                    if let Some(ref tx) = *EVENT_SENDER.read() {
                        let _ = tx.send(AppEvent::AltTabRelease);
                        let ui = UI_HWND.load(Ordering::Relaxed);
                        if ui != 0 {
                            let _ = PostMessageW(
                                HWND(ui as *mut _),
                                WM_APP_EVENT,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                }
            }
        } else if SEARCH_MODE.load(Ordering::Relaxed) && is_search_key(vk) {
            if is_down {
                if let Some(input) = decode_search_key(vk) {
                    SEARCH_INPUT.lock().push_back(input);
                }
            }
            return LRESULT(1);
        } else if enabled
            && vk == TRIGGER_VK.load(Ordering::Relaxed)
            && is_down
            && MOD_DOWN.load(Ordering::Relaxed)
        {
            if !foreground_is_fullscreen() {
                let shift = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                let direction = if shift { -1 } else { 1 };

                // If Alt chord, tap Ctrl so Alt doesn't activate menu bar
                if !use_ctrl {
                    keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                    keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                }

                ALTTAB_ACTIVE.store(true, Ordering::SeqCst);

                if let Some(ref tx) = *EVENT_SENDER.read() {
                    let _ = tx.send(AppEvent::AltTab { direction });
                    let ui = UI_HWND.load(Ordering::Relaxed);
                    if ui != 0 {
                        let _ =
                            PostMessageW(HWND(ui as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
                    }
                }

                return LRESULT(1); // Swallow so Windows' default switcher never fires
            }
        } else if enabled
            && !use_ctrl
            && vk == VK_OEM_3_BACKTICK
            && is_down
            && MOD_DOWN.load(Ordering::Relaxed)
        {
            // Alt+` cycles between windows of the foreground app (macOS-style).
            // Only bound for the Alt modifier: Ctrl+` is a common editor shortcut.
            if !foreground_is_fullscreen() {
                let shift = (GetAsyncKeyState(VK_SHIFT.0 as i32) as u16 & 0x8000) != 0;
                keybd_event(VK_CONTROL.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
                keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
                ALTTAB_ACTIVE.store(true, Ordering::SeqCst);
                post_event(AppEvent::AltTabSameApp {
                    direction: if shift { -1 } else { 1 },
                });
                return LRESULT(1);
            }
        } else if PICKER_MODE.load(Ordering::Relaxed) && is_picker_key(vk) {
            if is_down {
                PICKER_KEYS.lock().push_back(vk);
            }
            return LRESULT(1);
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}

const VK_OEM_3_BACKTICK: u32 = 0xC0;

fn post_event(event: AppEvent) {
    if let Some(ref tx) = *EVENT_SENDER.read() {
        let _ = tx.send(event);
        let ui = UI_HWND.load(Ordering::Relaxed);
        if ui != 0 {
            unsafe {
                let _ = PostMessageW(HWND(ui as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn install(
    event_sender: Sender<AppEvent>,
    ui_hwnd: HWND,
    use_ctrl: bool,
    trigger_vk: u32,
) -> Result<(), windows::core::Error> {
    *EVENT_SENDER.write() = Some(event_sender);
    UI_HWND.store(ui_hwnd.0 as usize, Ordering::SeqCst);
    USE_CTRL.store(use_ctrl, Ordering::SeqCst);
    TRIGGER_VK.store(trigger_vk, Ordering::SeqCst);

    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)?;
        *HOOK_HANDLE.lock() = Some(hook.0 as usize);
    }
    Ok(())
}

pub fn uninstall() {
    unsafe {
        if let Some(h) = HOOK_HANDLE.lock().take() {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
        }
    }
}

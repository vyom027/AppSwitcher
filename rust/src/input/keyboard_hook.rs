use crate::event::{AppEvent, WM_APP_EVENT};
use crossbeam_channel::Sender;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_LCONTROL,
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
static ALTTAB_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRIGGER_VK: AtomicU32 = AtomicU32::new(VK_TAB.0 as u32);
static USE_CTRL: AtomicBool = AtomicBool::new(false);
static UI_HWND: AtomicUsize = AtomicUsize::new(0);

static EVENT_SENDER: parking_lot::RwLock<Option<Sender<AppEvent>>> =
    parking_lot::RwLock::new(None);

static LAST_FG_HWND: AtomicU32 = AtomicU32::new(0);
static LAST_FG_FS: AtomicBool = AtomicBool::new(false);
static LAST_FG_TIME: parking_lot::Mutex<Option<Instant>> = parking_lot::Mutex::new(None);

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
        let mw = GetSystemMetrics(SM_CXSCREEN);
        let mh = GetSystemMetrics(SM_CYSCREEN);

        let is_fs = rect.left <= 0
            && rect.top <= 0
            && (rect.right - rect.left) >= mw
            && (rect.bottom - rect.top) >= mh;

        LAST_FG_HWND.store(hwnd_u32, Ordering::Relaxed);
        LAST_FG_FS.store(is_fs, Ordering::Relaxed);
        *LAST_FG_TIME.lock() = Some(now);
        is_fs
    }
}

unsafe extern "system" fn hook_proc(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
    if n_code >= 0 && ENABLED.load(Ordering::Relaxed) {
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
                            let _ = PostMessageW(HWND(ui as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
                        }
                    }
                }
            }
        } else if vk == TRIGGER_VK.load(Ordering::Relaxed) && is_down && MOD_DOWN.load(Ordering::Relaxed) {
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
                        let _ = PostMessageW(HWND(ui as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
                    }
                }

                return LRESULT(1); // Swallow so Windows' default switcher never fires
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
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

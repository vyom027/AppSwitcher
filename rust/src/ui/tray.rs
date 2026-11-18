use crate::event::{AppEvent, WM_APP_EVENT};
use crossbeam_channel::Sender;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, LoadIconW, PostMessageW, RegisterClassExW, SetForegroundWindow, TrackPopupMenuEx,
    MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_RBUTTONUP, WM_USER, WNDCLASSEXW,
};

pub const WM_TRAYICON: u32 = WM_USER + 101;
pub const ID_TRAY_SETTINGS: usize = 2001;
pub const ID_TRAY_QUIT: usize = 2002;
pub const ID_TRAY_OPEN_PICKER: usize = 2003;
pub const ID_TRAY_PAUSE: usize = 2004;
pub const ID_TRAY_VERSION: usize = 2005;

const TRAY_UID: u32 = 1;
const NIF_MESSAGE: u32 = 0x1;
const NIF_ICON: u32 = 0x2;
const NIF_TIP: u32 = 0x4;
const NIF_INFO: u32 = 0x10;
const NIIF_INFO: u32 = 0x1;
const NIIF_WARNING: u32 = 0x2;

static TRAY_EVENT_SENDER: parking_lot::RwLock<Option<Sender<AppEvent>>> =
    parking_lot::RwLock::new(None);
static TRAY_TARGET_HWND: AtomicUsize = AtomicUsize::new(0);
/// The tray's own message window, used by `notify` from any thread.
static TRAY_HWND: AtomicUsize = AtomicUsize::new(0);

pub struct TrayIcon {
    pub hwnd: HWND,
    nid: NOTIFYICONDATAW,
}

fn copy_wide(dst: &mut [u16], text: &str) {
    let max = dst.len().saturating_sub(1);
    let mut n = 0;
    for (i, c) in text.encode_utf16().take(max).enumerate() {
        dst[i] = c;
        n = i + 1;
    }
    dst[n] = 0;
}

fn base_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW::default();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid
}

fn update_tooltip() {
    let raw = TRAY_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let mut nid = base_nid(HWND(raw as *mut _));
    nid.uFlags = NOTIFY_ICON_DATA_FLAGS(NIF_TIP);
    copy_wide(&mut nid.szTip, &tooltip_text());
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn tooltip_text() -> String {
    if crate::input::is_paused() {
        format!("AppSwitcher {} (paused)", env!("CARGO_PKG_VERSION"))
    } else {
        format!("AppSwitcher {}", env!("CARGO_PKG_VERSION"))
    }
}

/// Show a tray balloon / Windows notification. Safe to call from any thread;
/// a no-op before the tray icon exists. Release builds have no console, so
/// this is how runtime problems reach the user.
pub fn notify(title: &str, message: &str, warning: bool) {
    let raw = TRAY_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        return;
    }
    let mut nid = base_nid(HWND(raw as *mut _));
    nid.uFlags = NOTIFY_ICON_DATA_FLAGS(NIF_INFO);
    nid.dwInfoFlags =
        windows::Win32::UI::Shell::NOTIFY_ICON_INFOTIP_FLAGS(if warning { NIIF_WARNING } else { NIIF_INFO });
    copy_wide(&mut nid.szInfoTitle, title);
    copy_wide(&mut nid.szInfo, message);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    let Ok(menu) = CreatePopupMenu() else { return };
    let version: Vec<u16> = format!("AppSwitcher {}\0", env!("CARGO_PKG_VERSION"))
        .encode_utf16()
        .collect();
    let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, ID_TRAY_VERSION, PCWSTR(version.as_ptr()));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_OPEN_PICKER, w!("Open switcher"));
    let pause_flags = if crate::input::is_paused() {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING | MENU_ITEM_FLAGS(0)
    };
    let _ = AppendMenuW(menu, pause_flags, ID_TRAY_PAUSE, w!("Pause gestures && Alt+Tab"));
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SETTINGS, w!("Settings…"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
    let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_QUIT, w!("Quit AppSwitcher"));

    // Required so the menu closes when the user clicks elsewhere.
    let _ = SetForegroundWindow(hwnd);
    let _ = TrackPopupMenuEx(
        menu,
        (TPM_LEFTALIGN | TPM_BOTTOMALIGN).0,
        pt.x,
        pt.y,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
}

unsafe extern "system" fn tray_window_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if msg == WM_TRAYICON {
        let event = l_param.0 as u32;
        if event == WM_RBUTTONUP {
            show_menu(hwnd);
        } else if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
            dispatch_tray_event(AppEvent::ShowSettings);
        }
    } else if msg == WM_COMMAND {
        let cmd = w_param.0 & 0xFFFF;
        match cmd {
            ID_TRAY_SETTINGS => dispatch_tray_event(AppEvent::ShowSettings),
            ID_TRAY_OPEN_PICKER => dispatch_tray_event(AppEvent::PreviewPicker),
            ID_TRAY_PAUSE => {
                let paused = !crate::input::is_paused();
                crate::input::set_paused(paused);
                update_tooltip();
                if paused {
                    notify(
                        "AppSwitcher paused",
                        "Gestures and Alt+Tab are passed through to Windows. Resume from the tray menu.",
                        false,
                    );
                }
            }
            ID_TRAY_QUIT => dispatch_tray_event(AppEvent::Quit),
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

fn dispatch_tray_event(event: AppEvent) {
    if let Some(ref tx) = *TRAY_EVENT_SENDER.read() {
        let _ = tx.send(event);
        let target = TRAY_TARGET_HWND.load(Ordering::Relaxed);
        if target != 0 {
            unsafe {
                let _ = PostMessageW(HWND(target as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

impl TrayIcon {
    pub fn new(
        event_sender: Sender<AppEvent>,
        ui_hwnd: HWND,
    ) -> Result<Self, windows::core::Error> {
        *TRAY_EVENT_SENDER.write() = Some(event_sender);
        TRAY_TARGET_HWND.store(ui_hwnd.0 as usize, Ordering::SeqCst);

        unsafe {
            let h_instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherTrayWnd");

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(tray_window_proc);
            wc.hInstance = h_instance.into();
            wc.lpszClassName = class_name;
            wc.hIcon = LoadIconW(h_instance, PCWSTR(1 as *const u16)).unwrap_or_default();
            wc.hIconSm = wc.hIcon;

            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("AppSwitcherTrayTarget"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND(std::ptr::null_mut()),
                None,
                h_instance,
                None,
            )?;

            let mut nid = base_nid(hwnd);
            nid.hIcon = wc.hIcon;
            nid.uFlags = NOTIFY_ICON_DATA_FLAGS(NIF_MESSAGE | NIF_ICON | NIF_TIP);
            nid.uCallbackMessage = WM_TRAYICON;
            copy_wide(&mut nid.szTip, &tooltip_text());

            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            TRAY_HWND.store(hwnd.0 as usize, Ordering::SeqCst);

            Ok(Self { hwnd, nid })
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        TRAY_HWND.store(0, Ordering::SeqCst);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

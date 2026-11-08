use crate::event::{AppEvent, WM_APP_EVENT};
use crossbeam_channel::Sender;
use std::mem::size_of;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetCursorPos, PostMessageW, RegisterClassExW, SetForegroundWindow, TrackPopupMenuEx,
    MF_SEPARATOR, MF_STRING, TPM_BOTTOMALIGN, TPM_LEFTALIGN, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_COMMAND, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_RBUTTONUP, WM_USER, WNDCLASSEXW,
};

pub const WM_TRAYICON: u32 = WM_USER + 101;
pub const ID_TRAY_SETTINGS: usize = 2001;
pub const ID_TRAY_QUIT: usize = 2002;

static TRAY_EVENT_SENDER: parking_lot::RwLock<Option<Sender<AppEvent>>> =
    parking_lot::RwLock::new(None);
static TRAY_TARGET_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct TrayIcon {
    pub hwnd: HWND,
    nid: NOTIFYICONDATAW,
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
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);

            if let Ok(menu) = CreatePopupMenu() {
                let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_SETTINGS, w!("Settings…"));
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, w!(""));
                let _ = AppendMenuW(menu, MF_STRING, ID_TRAY_QUIT, w!("Quit AppSwitcher"));

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
        } else if event == WM_LBUTTONUP || event == WM_LBUTTONDBLCLK {
            dispatch_tray_event(AppEvent::ShowSettings);
        }
    } else if msg == WM_COMMAND {
        let cmd = (w_param.0 & 0xFFFF) as usize;
        if cmd == ID_TRAY_SETTINGS {
            dispatch_tray_event(AppEvent::ShowSettings);
        } else if cmd == ID_TRAY_QUIT {
            dispatch_tray_event(AppEvent::Quit);
        }
    }
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

fn dispatch_tray_event(event: AppEvent) {
    if let Some(ref tx) = *TRAY_EVENT_SENDER.read() {
        let _ = tx.send(event);
        let target = TRAY_TARGET_HWND.load(std::sync::atomic::Ordering::Relaxed);
        if target != 0 {
            unsafe {
                let _ = PostMessageW(HWND(target as *mut _), WM_APP_EVENT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

impl TrayIcon {
    pub fn new(event_sender: Sender<AppEvent>, ui_hwnd: HWND) -> Result<Self, windows::core::Error> {
        *TRAY_EVENT_SENDER.write() = Some(event_sender);
        TRAY_TARGET_HWND.store(ui_hwnd.0 as usize, std::sync::atomic::Ordering::SeqCst);

        unsafe {
            let h_instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherTrayWnd");

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(tray_window_proc);
            wc.hInstance = h_instance.into();
            wc.lpszClassName = class_name;

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

            let mut nid = NOTIFYICONDATAW::default();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            nid.uFlags = NOTIFY_ICON_DATA_FLAGS(1 | 2 | 4); // NIF_MESSAGE | NIF_ICON | NIF_TIP
            nid.uCallbackMessage = WM_TRAYICON;

            let tip = "AppSwitcher Native (120Hz)";
            for (i, c) in tip.encode_utf16().enumerate().take(127) {
                nid.szTip[i] = c;
            }

            let _ = Shell_NotifyIconW(NIM_ADD, &nid);

            Ok(Self { hwnd, nid })
        }
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.nid);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_EVENT: u32 = WM_APP + 42;

#[derive(Debug, Clone)]
pub enum AppEvent {
    /// 3-finger horizontal swipe: +1 for right, -1 for left
    Swipe(i32),
    /// 3-finger stillness hold dwell reached: open picker
    Hold,
    /// 3-finger pinch inward
    Pinch,
    /// Alt+Tab hotkey chord pressed (+1 = next, -1 = prev with shift)
    AltTab { direction: i32 },
    /// Alt/Ctrl modifier released after Alt+Tab
    AltTabRelease,
    /// Open the Modern Settings Dialog
    ShowSettings,
    /// Exit the application cleanly
    Quit,
}


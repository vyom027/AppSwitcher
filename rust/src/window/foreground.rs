use crate::window::enumerator::invalidate_window_cache;
use std::mem::size_of;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    SetForegroundWindow, ShowWindow, SwitchToThisWindow, SW_RESTORE,
};

pub fn is_own_window(hwnd: HWND) -> bool {
    unsafe {
        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

pub fn nudge_cursor() {
    unsafe {
        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: 1,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx: -1,
                        dy: 0,
                        mouseData: 0,
                        dwFlags: MOUSEEVENTF_MOVE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        let _ = SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

pub fn bring_to_front(hwnd: HWND) {
    if hwnd.0.is_null() {
        return;
    }
    invalidate_window_cache();

    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }

        let fg = GetForegroundWindow();
        let cur_thread = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);

        let mut attached = false;
        if fg_thread != 0 && fg_thread != cur_thread {
            attached = AttachThreadInput(fg_thread, cur_thread, true).as_bool();
        }

        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);

        if attached {
            let _ = AttachThreadInput(fg_thread, cur_thread, false);
        }

        if GetForegroundWindow() != hwnd {
            SwitchToThisWindow(hwnd, true);
        }
    }
}

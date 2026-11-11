#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animations;
mod config;
mod event;
mod input;
mod layouts;
mod render;
mod ui;
mod window;

use animations::TransitionKind;
use config::Settings;
use event::{AppEvent, WM_APP_EVENT};
use input::{run_input_thread, stop_input_thread};
use std::sync::Arc;
use std::thread;
use ui::{OverlayWindow, PickerMode, SettingsDialog, TrayIcon};
use window::{
    bring_to_front, capture::ImageBuffer, capture_window_image, enumerator::WindowInfo,
    get_open_windows, nudge_cursor, sort_by_mru, start_mru_watcher,
};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, GetAsyncKeyState, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DisableProcessWindowsGhosting, DispatchMessageW, GetMessageW, PeekMessageW, PostMessageW,
    PostQuitMessage, TranslateMessage, MSG, PM_REMOVE,
};

static IS_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const MUTEX_NAME: PCWSTR = w!("AppSwitcher_Rust_Running_Mutex");
const SHOW_SETTINGS_EVENT: PCWSTR = w!("AppSwitcher_Rust_ShowSettings_Event");
const PY_MUTEX_NAME: PCWSTR = w!("AppSwitcher_Running_Mutex");

struct SingleInstanceGuard {
    handle: HANDLE,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    unsafe {
        let handle = CreateMutexW(None, false, MUTEX_NAME).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(SingleInstanceGuard { handle })
    }
}

fn check_python_instance() -> bool {
    unsafe {
        if let Ok(handle) = CreateMutexW(None, false, PY_MUTEX_NAME) {
            let exists = GetLastError() == ERROR_ALREADY_EXISTS;
            let _ = CloseHandle(handle);
            exists
        } else {
            false
        }
    }
}

fn signal_existing_instance() {
    unsafe {
        if let Ok(handle) = CreateEventW(None, false, false, SHOW_SETTINGS_EVENT) {
            let _ = SetEvent(handle);
            let _ = CloseHandle(handle);
        }
    }
}

fn commit_window_switch(
    overlay: &OverlayWindow,
    settings: &parking_lot::RwLock<Settings>,
    mru: &[WindowInfo],
    sel: usize,
    opt_curr_img: Option<ImageBuffer>,
    opt_next_img: Option<ImageBuffer>,
) {
    if sel == 0 || sel >= mru.len() {
        bring_to_front(mru[0].hwnd);
        nudge_cursor();
        return;
    }

    let (anim_name, duration) = {
        let cfg = settings.read();
        (cfg.animation.clone(), cfg.duration as f32)
    };
    let anim_kind = TransitionKind::from_str(&anim_name);
    let curr_hwnd = mru[0].hwnd;
    let target_hwnd = mru[sel].hwnd;

    let curr_img = opt_curr_img.or_else(|| capture_window_image(curr_hwnd));
    let next_img = opt_next_img.or_else(|| capture_window_image(target_hwnd));

    if let (Some(c), Some(n)) = (curr_img, next_img) {
        let dir = if sel == 1 { 1 } else { -1 };
        overlay.run_switch_transition(dir, &c, &n, target_hwnd, anim_kind, duration, true);
    } else {
        bring_to_front(target_hwnd);
        nudge_cursor();
    }
}

fn send_shortcut(mods: &[u32], vk: u32) {
    unsafe {
        for &m in mods {
            keybd_event(m as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        }
        keybd_event(vk as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
        keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
        for &m in mods.iter().rev() {
            keybd_event(m as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
}

fn main() {
    // 1. Enable Windows 11 Per-Monitor V2 DPI awareness & disable OS ghosting
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        DisableProcessWindowsGhosting();
    }

    // 2. Single-instance check
    let _guard = match acquire_single_instance() {
        Some(g) => g,
        None => {
            signal_existing_instance();
            println!("[exit] AppSwitcher is already running — surfaced Settings window.");
            return;
        }
    };

    println!("==========================================================");
    println!("  AppSwitcher Native Rust v2.0 (Direct2D 120Hz GPU Engine)");
    println!("  * Alt+Tab (Keyboard)         -> Ultra-smooth app picker ");
    println!("  * 3-finger swipe (horizontal) -> 120Hz switch transition");
    println!("  * 3-finger hold (~0.2s)       -> Interactive card picker");
    println!("  * 3-finger pinch inward       -> Custom shortcut chord  ");
    println!("  * System Tray icon            -> Settings & Quit options");
    println!("  Press Ctrl+C in console to stop                         ");
    println!("==========================================================");

    if check_python_instance() {
        println!("[warning] An instance of Python AppSwitcher was detected running.");
        println!("          Close it from System Tray or Task Manager to ensure full control of gestures/Alt+Tab.");
    }

    // 3. Load user settings
    let settings = Arc::new(parking_lot::RwLock::new(Settings::load()));
    println!("[config] Loaded configuration from %APPDATA%\\AppSwitcher");

    // 4. Start MRU focus recency watcher
    start_mru_watcher();

    // 5. Create async event channel between Background Threads and Main UI Thread
    let (event_tx, event_rx) = crossbeam_channel::unbounded::<AppEvent>();

    // 6. Initialize Overlay Window & Settings Dialog on Main UI Thread
    let overlay = match OverlayWindow::new() {
        Ok(ov) => Arc::new(ov),
        Err(e) => {
            eprintln!("[fatal] Direct2D overlay initialization failed: {:?}", e);
            return;
        }
    };

    let mut settings_dialog = match SettingsDialog::new(
        Arc::clone(&settings),
        event_tx.clone(),
        overlay.hwnd.0 as usize,
    ) {
        Ok(dlg) => dlg,
        Err(e) => {
            eprintln!("[fatal] Settings dialog initialization failed: {:?}", e);
            return;
        }
    };

    // 7. Initialize System Tray on Main UI Thread
    let _tray = TrayIcon::new(event_tx.clone(), overlay.hwnd).ok();

    // 8. Watch for "Show Settings" event from secondary launches
    let tx_sig = event_tx.clone();
    let ov_hwnd_usize = overlay.hwnd.0 as usize;
    thread::spawn(move || unsafe {
        if let Ok(event) = CreateEventW(None, false, false, SHOW_SETTINGS_EVENT) {
            loop {
                if WaitForSingleObject(event, INFINITE) == windows::Win32::Foundation::WAIT_OBJECT_0
                {
                    let _ = tx_sig.send(AppEvent::ShowSettings);
                    let _ = PostMessageW(
                        HWND(ov_hwnd_usize as *mut _),
                        WM_APP_EVENT,
                        WPARAM(0),
                        LPARAM(0),
                    );
                }
            }
        }
    });

    // 9. Start dedicated Background Input Thread (Pumps RawInput & Keyboard Hook)
    let tx_input = event_tx.clone();
    let (use_ctrl, trigger_vk, sensitivity, alt_tab_enabled) = {
        let cfg = settings.read();
        (
            cfg.hotkey_mod.eq_ignore_ascii_case("ctrl"),
            cfg.hotkey_key,
            cfg.sensitivity,
            cfg.alt_tab,
        )
    };
    crate::input::keyboard_hook::set_enabled(alt_tab_enabled);
    thread::spawn(move || {
        if let Err(e) = run_input_thread(tx_input, ov_hwnd_usize, use_ctrl, trigger_vk, sensitivity)
        {
            eprintln!("[error] Background input thread terminated: {:?}", e);
        }
    });

    // 10. Console Ctrl+C handler
    let tx_ctrlc = event_tx.clone();
    ctrlc_handler(move || {
        let _ = tx_ctrlc.send(AppEvent::Quit);
        unsafe {
            let _ = PostMessageW(
                HWND(ov_hwnd_usize as *mut _),
                WM_APP_EVENT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    });

    // 11. Main UI Windows Message Loop
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_APP_EVENT {
                while let Ok(event) = event_rx.try_recv() {
                    match event {
                        AppEvent::AltTab { direction } => {
                            let (blocklist, layout_name, accent_rgb, use_ctrl, sensitivity) = {
                                let cfg = settings.read();
                                (
                                    cfg.blocklist.clone(),
                                    cfg.layout.clone(),
                                    cfg.accent,
                                    cfg.hotkey_mod.eq_ignore_ascii_case("ctrl"),
                                    cfg.sensitivity as f32,
                                )
                            };
                            let mru = sort_by_mru(get_open_windows(&blocklist, false));
                            if mru.len() >= 2 {
                                let start_idx = if direction > 0 { 1 } else { mru.len() - 1 };

                                // Quick Alt+Tab detection (matches Python / Windows native behavior):
                                // If user quickly taps Alt+Tab (<100ms), switch immediately without opening or flashing the picker!
                                let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
                                let mut quick_tap = false;
                                let wait_start = std::time::Instant::now();
                                while wait_start.elapsed() < std::time::Duration::from_millis(100) {
                                    let is_mod_down =
                                        (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0;
                                    if !is_mod_down {
                                        quick_tap = true;
                                        break;
                                    }
                                    let mut pmsg = MSG::default();
                                    while PeekMessageW(&mut pmsg, None, 0, 0, PM_REMOVE).as_bool() {
                                        if pmsg.message == WM_APP_EVENT {
                                            while let Ok(ev) = event_rx.try_recv() {
                                                if matches!(ev, AppEvent::AltTabRelease) {
                                                    quick_tap = true;
                                                    break;
                                                }
                                            }
                                        }
                                        let _ = TranslateMessage(&pmsg);
                                        DispatchMessageW(&pmsg);
                                    }
                                    if quick_tap {
                                        break;
                                    }
                                    thread::sleep(std::time::Duration::from_millis(4));
                                }

                                if quick_tap {
                                    commit_window_switch(
                                        &overlay, &settings, &mru, start_idx, None, None,
                                    );
                                    continue;
                                }

                                let accent = D2D1_COLOR_F {
                                    r: accent_rgb[0] as f32 / 255.0,
                                    g: accent_rgb[1] as f32 / 255.0,
                                    b: accent_rgb[2] as f32 / 255.0,
                                    a: 1.0,
                                };
                                if let Some((sel, c_img, n_img)) = overlay.run_picker(
                                    &mru,
                                    start_idx,
                                    &layout_name,
                                    accent,
                                    PickerMode::AltTab,
                                    use_ctrl,
                                    sensitivity,
                                    &event_rx,
                                ) {
                                    commit_window_switch(
                                        &overlay, &settings, &mru, sel, c_img, n_img,
                                    );
                                }
                            }
                        }
                        AppEvent::PreviewPicker => {
                            let (blocklist, layout_name, accent_rgb, use_ctrl, sensitivity) = {
                                let cfg = settings.read();
                                (
                                    cfg.blocklist.clone(),
                                    cfg.layout.clone(),
                                    cfg.accent,
                                    cfg.hotkey_mod.eq_ignore_ascii_case("ctrl"),
                                    cfg.sensitivity as f32,
                                )
                            };
                            let mru = sort_by_mru(get_open_windows(&blocklist, false));
                            if mru.len() >= 2 {
                                let accent = D2D1_COLOR_F {
                                    r: accent_rgb[0] as f32 / 255.0,
                                    g: accent_rgb[1] as f32 / 255.0,
                                    b: accent_rgb[2] as f32 / 255.0,
                                    a: 1.0,
                                };
                                if let Some((sel, c_img, n_img)) = overlay.run_picker(
                                    &mru,
                                    0,
                                    &layout_name,
                                    accent,
                                    PickerMode::Preview,
                                    use_ctrl,
                                    sensitivity,
                                    &event_rx,
                                ) {
                                    commit_window_switch(
                                        &overlay, &settings, &mru, sel, c_img, n_img,
                                    );
                                }
                            }
                        }
                        AppEvent::PreviewAnimation => {
                            let (blocklist, animation, duration) = {
                                let cfg = settings.read();
                                (
                                    cfg.blocklist.clone(),
                                    cfg.animation.clone(),
                                    cfg.duration as f32,
                                )
                            };
                            let mru = sort_by_mru(get_open_windows(&blocklist, false));
                            if mru.len() >= 2 {
                                if let (Some(curr), Some(next)) = (
                                    capture_window_image(mru[0].hwnd),
                                    capture_window_image(mru[1].hwnd),
                                ) {
                                    overlay.run_switch_transition(
                                        1,
                                        &curr,
                                        &next,
                                        mru[1].hwnd,
                                        TransitionKind::from_str(&animation),
                                        duration,
                                        false,
                                    );
                                }
                            }
                        }
                        AppEvent::Hold => {
                            let (blocklist, layout_name, accent_rgb, use_ctrl, sensitivity) = {
                                let cfg = settings.read();
                                (
                                    cfg.blocklist.clone(),
                                    cfg.layout.clone(),
                                    cfg.accent,
                                    cfg.hotkey_mod.eq_ignore_ascii_case("ctrl"),
                                    cfg.sensitivity as f32,
                                )
                            };
                            let mru = sort_by_mru(get_open_windows(&blocklist, false));
                            if mru.len() >= 2 {
                                let accent = D2D1_COLOR_F {
                                    r: accent_rgb[0] as f32 / 255.0,
                                    g: accent_rgb[1] as f32 / 255.0,
                                    b: accent_rgb[2] as f32 / 255.0,
                                    a: 1.0,
                                };
                                if let Some((sel, c_img, n_img)) = overlay.run_picker(
                                    &mru,
                                    0,
                                    &layout_name,
                                    accent,
                                    PickerMode::Touchpad,
                                    use_ctrl,
                                    sensitivity,
                                    &event_rx,
                                ) {
                                    commit_window_switch(
                                        &overlay, &settings, &mru, sel, c_img, n_img,
                                    );
                                }
                            }
                        }
                        AppEvent::Swipe(direction) => {
                            if IS_BUSY.swap(true, std::sync::atomic::Ordering::SeqCst) {
                                continue;
                            }
                            struct BusyGuard;
                            impl Drop for BusyGuard {
                                fn drop(&mut self) {
                                    IS_BUSY.store(false, std::sync::atomic::Ordering::SeqCst);
                                }
                            }
                            let _busy = BusyGuard;

                            let (blocklist, anim_name, duration) = {
                                let cfg = settings.read();
                                (
                                    cfg.blocklist.clone(),
                                    cfg.animation.clone(),
                                    cfg.duration as f32,
                                )
                            };
                            let mru = sort_by_mru(get_open_windows(&blocklist, false));
                            if mru.len() < 2 {
                                // Exactly 1 window or none: no switch, zero change
                                continue;
                            }

                            let anim_kind = TransitionKind::from_str(&anim_name);
                            let target_idx = if direction > 0 { 1 } else { mru.len() - 1 };
                            let curr_hwnd = mru[0].hwnd;
                            let target_hwnd = mru[target_idx].hwnd;

                            let curr_img = capture_window_image(curr_hwnd);
                            let next_img = capture_window_image(target_hwnd);

                            if let (Some(c), Some(n)) = (curr_img, next_img) {
                                overlay.run_switch_transition(
                                    direction,
                                    &c,
                                    &n,
                                    target_hwnd,
                                    anim_kind,
                                    duration,
                                    true,
                                );
                            } else {
                                bring_to_front(target_hwnd);
                                nudge_cursor();
                            }

                            // The transition loop drains Windows notifications to
                            // stay responsive. Keep channel events intact here:
                            // matching only Swipe used to consume and discard the
                            // first non-swipe event (including Quit/settings).
                        }
                        AppEvent::Pinch => {
                            let cfg = settings.read();
                            if cfg.pinch_enabled && cfg.pinch_vk > 0 {
                                send_shortcut(&cfg.pinch_mods, cfg.pinch_vk);
                            }
                        }
                        AppEvent::ShowSettings => {
                            settings_dialog.show();
                        }
                        AppEvent::Quit => {
                            stop_input_thread();
                            PostQuitMessage(0);
                        }
                        _ => {}
                    }
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    println!("\n[exit] AppSwitcher Rust engine stopped.");
}

fn ctrlc_handler<F: Fn() + Send + Sync + 'static>(handler: F) {
    let handler = Arc::new(handler);
    unsafe {
        windows::Win32::System::Console::SetConsoleCtrlHandler(Some(console_ctrl_handler), true)
            .ok();
    }
    *CTRL_HANDLER.lock() = Some(handler);
}

static CTRL_HANDLER: parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>> =
    parking_lot::Mutex::new(None);

unsafe extern "system" fn console_ctrl_handler(
    dw_ctrl_type: u32,
) -> windows::Win32::Foundation::BOOL {
    if dw_ctrl_type == windows::Win32::System::Console::CTRL_C_EVENT {
        if let Some(ref h) = *CTRL_HANDLER.lock() {
            h();
        }
        return windows::Win32::Foundation::BOOL(1);
    }
    windows::Win32::Foundation::BOOL(0)
}

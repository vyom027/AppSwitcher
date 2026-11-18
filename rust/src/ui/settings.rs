use crate::config::Settings;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::layouts::draw_liquid_glass;
use crate::render::d2d::{glyph, Align, D2DContext, Font};
use crossbeam_channel::Sender;
use std::mem::size_of;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, RedrawWindow, PAINTSTRUCT,
    RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, LoadIconW, PostMessageW,
    RegisterClassExW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    HTCAPTION, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_CLOSE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCLBUTTONDOWN, WM_PAINT,
    WM_SYSKEYDOWN, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_POPUP,
};
/// Sent after TrackMouseEvent(TME_LEAVE) when the cursor leaves the window.
const WM_MOUSELEAVE: u32 = 0x02A3;
const ANIMATIONS: &[(&str, &str)] = &[
    ("slide", "Slide"),
    ("duo_fold", "Duo Fold"),
    ("fade", "Fade"),
    ("zoom", "Zoom"),
    ("cube", "Cube"),
    ("flip", "Flip"),
    ("push", "Push"),
    ("stack", "Stack"),
];
const LAYOUTS: &[(&str, &str)] = &[
    ("dock", "Dock"),
    ("row", "Row"),
    ("hero", "Hero"),
    ("expose_cloud", "Exposé Cloud"),
    ("spotlight", "Spotlight"),
];
const HOTKEYS: &[(&str, u32)] = &[
    ("Tab", 0x09),
    ("`", 0xC0),
    ("Space", 0x20),
    ("Q", 0x51),
    ("E", 0x45),
    ("Z", 0x5A),
];
const ACCENTS: &[(&str, [u8; 3])] = &[
    ("Blue", [150, 205, 255]),
    ("Indigo", [124, 156, 255]),
    ("Violet", [185, 140, 255]),
    ("Rose", [255, 140, 198]),
    ("Mint", [91, 224, 176]),
    ("Amber", [255, 212, 121]),
    ("Coral", [255, 138, 101]),
];
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
fn three_finger_enabled() -> bool {
    let output = Command::new("reg.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad",
            "/v",
            "ThreeFingerSlideEnabled",
        ])
        .output();
    let Ok(output) = output else { return true };
    if !output.status.success() {
        return true;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .last()
        .and_then(|value| value.strip_prefix("0x"))
        .and_then(|value| u32::from_str_radix(value, 16).ok())
        .map(|value| value != 0)
        .unwrap_or(true)
}
pub struct SettingsDialog {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
    pub d2d: D2DContext,
    pub settings: Arc<parking_lot::RwLock<Settings>>,
    pub event_tx: Sender<AppEvent>,
    pub main_hwnd: usize,
    status: String,
    open_titles: Vec<String>,
    window_page: usize,
    capture_pinch: bool,
    autostart: bool,
    touchpad_warning: bool,
    open_combo: Option<usize>,
    /// Cursor position in client coordinates while over the window (hover states).
    mouse: Option<(f32, f32)>,
}
unsafe impl Send for SettingsDialog {}
unsafe impl Sync for SettingsDialog {}
fn inside(r: D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}
fn panels(w: u32) -> [D2D_RECT_F; 3] {
    let gap = 14.0;
    let x = 24.0;
    let width = (w as f32 - 2.0 * x - 2.0 * gap) / 3.0;
    std::array::from_fn(|i| {
        let left = x + i as f32 * (width + gap);
        D2D_RECT_F {
            left,
            top: 140.0,
            right: left + width,
            bottom: 0.0,
        }
    })
}
fn panel_y(cfg: &Settings) -> f32 {
    if cfg.warn_dismissed {
        92.0
    } else {
        158.0
    }
}
fn animation_rect(i: usize, r: D2D_RECT_F) -> D2D_RECT_F {
    let gap = 7.0;
    let width = (r.right - r.left - 32.0 - gap * 3.0) / 4.0;
    let left = r.left + 16.0 + (i % 4) as f32 * (width + gap);
    let top = r.top + 79.0 + (i / 4) as f32 * 43.0;
    D2D_RECT_F {
        left,
        top,
        right: left + width,
        bottom: top + 36.0,
    }
}
fn layout_rect(i: usize, r: D2D_RECT_F) -> D2D_RECT_F {
    let top = r.top + 196.0 + i as f32 * 44.0;
    D2D_RECT_F {
        left: r.left + 16.0,
        top,
        right: r.right - 16.0,
        bottom: top + 38.0,
    }
}
fn slider_rect(i: usize, r: D2D_RECT_F) -> D2D_RECT_F {
    let top = r.top + 86.0 + i as f32 * 82.0;
    D2D_RECT_F {
        left: r.left + 22.0,
        top: top + 35.0,
        right: r.right - 22.0,
        bottom: top + 43.0,
    }
}
fn set_autostart(on: bool) -> bool {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let value = format!("\"{}\"", exe.display());
        Command::new("reg.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args([
                "add",
                key,
                "/v",
                "AppSwitcher",
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else {
        Command::new("reg.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["delete", key, "/v", "AppSwitcher", "/f"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}
fn autostart_enabled() -> bool {
    Command::new("reg.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "AppSwitcher",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
fn key_name(vk: u32) -> &'static str {
    HOTKEYS
        .iter()
        .find(|(_, code)| *code == vk)
        .map(|(name, _)| *name)
        .unwrap_or("Tab")
}
fn dashboard_panels(width: u32, height: u32, warning: bool) -> [D2D_RECT_F; 3] {
    let x = 30.0;
    let gap = 18.0;
    let col = (width as f32 - x * 2.0 - gap * 2.0) / 3.0;
    let top = if warning { 158.0 } else { 96.0 };
    let bottom = height as f32 - 102.0;
    std::array::from_fn(|i| {
        let left = x + i as f32 * (col + gap);
        D2D_RECT_F {
            left,
            top,
            right: left + col,
            bottom,
        }
    })
}
fn look_combo(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let top = panel.top + if index == 0 { 76.0 } else { 154.0 };
    D2D_RECT_F {
        left: panel.left + 20.0,
        top,
        right: panel.right - 20.0,
        bottom: top + 42.0,
    }
}
fn hotkey_combo(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let gap = 10.0;
    let half = (panel.right - panel.left - 40.0 - gap) * 0.5;
    let left = panel.left + 20.0 + index as f32 * (half + gap);
    D2D_RECT_F {
        left,
        top: panel.top + 232.0,
        right: left + half,
        bottom: panel.top + 274.0,
    }
}
fn pinch_capture_rect(panel: D2D_RECT_F) -> D2D_RECT_F {
    D2D_RECT_F {
        left: panel.left + 20.0,
        top: panel.top + 326.0,
        right: panel.right - 86.0,
        bottom: panel.top + 368.0,
    }
}
fn pinch_clear_rect(panel: D2D_RECT_F) -> D2D_RECT_F {
    let r = pinch_capture_rect(panel);
    D2D_RECT_F {
        left: r.right + 10.0,
        top: r.top,
        right: panel.right - 20.0,
        bottom: r.bottom,
    }
}
fn preview_button(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let gap = 10.0;
    let width = (panel.right - panel.left - 40.0 - gap) * 0.5;
    let left = panel.left + 20.0 + index as f32 * (width + gap);
    let top = panel.bottom - 54.0;
    D2D_RECT_F {
        left,
        top,
        right: left + width,
        bottom: top + 40.0,
    }
}
fn slider_track(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let top = panel.top + 74.0 + index as f32 * 82.0;
    D2D_RECT_F {
        left: panel.left + 24.0,
        top,
        right: panel.right - 24.0,
        bottom: top + 6.0,
    }
}
fn accent_swatch(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let left = panel.left + 102.0 + index as f32 * 32.0;
    let top = panel.bottom - 43.0;
    D2D_RECT_F {
        left,
        top,
        right: left + 24.0,
        bottom: top + 24.0,
    }
}
fn window_row(panel: D2D_RECT_F, index: usize) -> D2D_RECT_F {
    let top = panel.top + 58.0 + index as f32 * 44.0;
    D2D_RECT_F {
        left: panel.left + 12.0,
        top,
        right: panel.right - 12.0,
        bottom: top + 40.0,
    }
}
fn footer_switch(index: usize, width: u32) -> D2D_RECT_F {
    let left = if index == 0 { 270.0 } else { 550.0 };
    let _ = width;
    D2D_RECT_F {
        left,
        top: 0.0,
        right: left + 50.0,
        bottom: 30.0,
    }
}
fn save_button(width: u32, height: u32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: width as f32 - 160.0,
        top: height as f32 - 64.0,
        right: width as f32 - 30.0,
        bottom: height as f32 - 20.0,
    }
}
unsafe extern "system" fn settings_wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr != 0 {
        let dlg = &mut *(ptr as *mut SettingsDialog);
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let _ = BeginPaint(hwnd, &mut ps);
                dlg.render();
                let _ = EndPaint(hwnd, &ps);
                return LRESULT(0);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_KEYDOWN | WM_SYSKEYDOWN if dlg.capture_pinch => {
                let vk = w.0 as u32;
                if vk == 0x1b {
                    dlg.capture_pinch = false;
                    dlg.status = "Shortcut capture cancelled.".into();
                } else if !matches!(vk, 0x10 | 0x11 | 0x12 | 0x5b | 0x5c) {
                    let mut s = dlg.settings.write();
                    s.pinch_mods.clear();
                    for (k, c) in [(0x11, 0x11), (0x12, 0x12), (0x10, 0x10), (0x5b, 0x5b)] {
                        if (GetAsyncKeyState(k) as u16 & 0x8000) != 0 {
                            s.pinch_mods.push(c);
                        }
                    }
                    s.pinch_vk = vk;
                    let _ = s.save();
                    dlg.capture_pinch = false;
                    dlg.status = "Pinch shortcut updated.".into();
                }
                if !dlg.capture_pinch {
                    crate::input::keyboard_hook::set_enabled(dlg.settings.read().alt_tab);
                }
                dlg.render();
                let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
                return LRESULT(0);
            }
            WM_MOUSEMOVE => {
                let x = (l.0 as usize & 0xffff) as i16 as f32;
                let y = ((l.0 as usize >> 16) & 0xffff) as i16 as f32;
                if dlg.mouse.is_none() {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                dlg.mouse = Some((x, y));
                dlg.render();
                return LRESULT(0);
            }
            WM_MOUSELEAVE => {
                dlg.mouse = None;
                dlg.render();
                return LRESULT(0);
            }
            WM_LBUTTONDOWN => {
                let x = (l.0 as usize & 0xffff) as i16 as f32;
                let y = ((l.0 as usize >> 16) & 0xffff) as i16 as f32;
                if y < 68.0 {
                    if x > dlg.width as f32 - 72.0 {
                        let _ = ShowWindow(hwnd, SW_HIDE);
                    } else {
                        let _ = ReleaseCapture();
                        let _ = SendMessageW(
                            hwnd,
                            WM_NCLBUTTONDOWN,
                            WPARAM(HTCAPTION as usize),
                            LPARAM(0),
                        );
                    }
                    return LRESULT(0);
                }
                dlg.handle_click(x, y);
                dlg.render();
                let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
                return LRESULT(0);
            }
            windows::Win32::UI::WindowsAndMessaging::WM_SIZE => {
                let width = (l.0 & 0xffff) as u32;
                let height = ((l.0 >> 16) & 0xffff) as u32;
                if width > 0 && height > 0 {
                    dlg.width = width;
                    dlg.height = height;
                    let _ = dlg.d2d.resize(width, height);
                    dlg.render();
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                if dlg.capture_pinch {
                    dlg.capture_pinch = false;
                    crate::input::keyboard_hook::set_enabled(dlg.settings.read().alt_tab);
                }
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w, l)
}
impl SettingsDialog {
    pub fn new(
        settings: Arc<parking_lot::RwLock<Settings>>,
        event_tx: Sender<AppEvent>,
        main_hwnd: usize,
    ) -> Result<Box<Self>, windows::core::Error> {
        unsafe {
            let (instance, width, height) = (GetModuleHandleW(None)?, 1200, 720);
            let class_name = w!("AppSwitcherSettingsWndClass");
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(settings_wnd_proc);
            wc.hInstance = instance.into();
            wc.lpszClassName = class_name;
            wc.hIcon = LoadIconW(instance, PCWSTR(1 as *const u16)).unwrap_or_default();
            wc.hIconSm = wc.hIcon;
            let _ = RegisterClassExW(&wc);
            let style = WS_POPUP;
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                class_name,
                w!("AppSwitcher — Settings"),
                style,
                120,
                120,
                width,
                height,
                HWND(std::ptr::null_mut()),
                None,
                instance,
                None,
            )?;
            let dark = 1i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20),
                &dark as *const _ as *const _,
                size_of::<i32>() as u32,
            );
            let rounded = 2i32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(33),
                &rounded as *const _ as *const _,
                size_of::<i32>() as u32,
            );
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let width = (client.right - client.left).max(1000) as u32;
            let height = (client.bottom - client.top).max(600) as u32;
            let d2d = D2DContext::new(hwnd, width, height)?;
            let mut dlg = Box::new(Self {
                hwnd,
                width,
                height,
                d2d,
                settings,
                event_tx,
                main_hwnd,
                status: "Changes save as you go.".into(),
                open_titles: Vec::new(),
                window_page: 0,
                capture_pinch: false,
                autostart: autostart_enabled(),
                touchpad_warning: three_finger_enabled(),
                open_combo: None,
                mouse: None,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, dlg.as_mut() as *mut _ as isize);
            Ok(dlg)
        }
    }
    fn refresh_windows(&mut self) {
        self.open_titles = crate::window::get_open_windows(&[], true)
            .into_iter()
            .map(|w| w.title)
            .collect();
        for t in self.settings.read().blocklist.clone() {
            if !self.open_titles.contains(&t) {
                self.open_titles.push(t);
            }
        }
        self.window_page = self
            .window_page
            .min(self.open_titles.len().saturating_sub(1) / 10);
    }
    pub fn show(&mut self) {
        self.refresh_windows();
        self.touchpad_warning = three_finger_enabled();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetWindowPos(self.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = RedrawWindow(self.hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
        }
        self.render();
    }
    fn persist(&mut self, msg: &str) {
        // Push live-tunable values to the input thread so they apply
        // immediately instead of after a restart.
        crate::input::set_sensitivity(self.settings.read().sensitivity);
        self.status = if self.settings.read().save().is_ok() {
            msg.into()
        } else {
            "Could not save settings.".into()
        };
    }
    fn notify_main(&self) {
        unsafe {
            let _ = PostMessageW(
                HWND(self.main_hwnd as *mut _),
                WM_APP_EVENT,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
    #[allow(dead_code)]
    fn handle_click_legacy(&mut self, x: f32, y: f32) {
        let cfg = self.settings.read().clone();
        let mut p = panels(self.width);
        for q in &mut p {
            q.top = panel_y(&cfg);
            q.bottom = self.height as f32 - 102.0;
        }
        if self.touchpad_warning && !cfg.warn_dismissed && (75.0..=140.0).contains(&y) {
            if x > self.width as f32 - 178.0 {
                self.settings.write().warn_dismissed = true;
                self.persist("Gesture notice dismissed.");
            } else if x > self.width as f32 - 330.0 {
                unsafe {
                    let _ = ShellExecuteW(
                        self.hwnd,
                        w!("open"),
                        w!("ms-settings:devices-touchpad"),
                        None,
                        None,
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                    );
                }
            }
            return;
        }
        if y > self.height as f32 - 92.0 {
            if (360.0..=592.0).contains(&x) {
                let mut s = self.settings.write();
                s.alt_tab = !s.alt_tab;
                let on = s.alt_tab;
                drop(s);
                crate::input::keyboard_hook::set_enabled(on);
                self.persist(if on {
                    "Alt+Tab replacement enabled."
                } else {
                    "Alt+Tab replacement disabled."
                });
            } else if (610.0..=842.0).contains(&x) {
                let on = !self.autostart;
                self.status = if set_autostart(on) {
                    self.autostart = on;
                    if on {
                        "AppSwitcher will start with Windows.".into()
                    } else {
                        "Windows startup disabled.".into()
                    }
                } else {
                    "Could not change Windows startup setting.".into()
                };
            } else if x >= self.width as f32 - 156.0 {
                self.persist("All settings saved.");
            }
            return;
        }
        if inside(p[0], x, y) {
            for (i, (name, _)) in ANIMATIONS.iter().enumerate() {
                if inside(animation_rect(i, p[0]), x, y) {
                    self.settings.write().animation = (*name).into();
                    self.persist("Animation updated.");
                    return;
                }
            }
            for (i, (name, _)) in LAYOUTS.iter().enumerate() {
                if inside(layout_rect(i, p[0]), x, y) {
                    self.settings.write().layout = (*name).into();
                    self.persist("Picker layout updated.");
                    return;
                }
            }
            let gap = 8.0;
            let half = (p[0].right - p[0].left - 40.0 - gap) * 0.5;
            let layout_preview = D2D_RECT_F {
                left: p[0].left + 16.0,
                top: p[0].bottom - 47.0,
                right: p[0].left + 16.0 + half,
                bottom: p[0].bottom - 12.0,
            };
            let animation_preview = D2D_RECT_F {
                left: layout_preview.right + gap,
                top: layout_preview.top,
                right: p[0].right - 16.0,
                bottom: layout_preview.bottom,
            };
            if inside(layout_preview, x, y) {
                let _ = self.event_tx.send(AppEvent::PreviewPicker);
                self.notify_main();
                self.status = "Picker preview sent.".into();
            } else if inside(animation_preview, x, y) {
                let _ = self.event_tx.send(AppEvent::PreviewAnimation);
                self.notify_main();
                self.status = "Animation preview sent.".into();
            }
            return;
        }
        if inside(p[1], x, y) {
            for i in 0..3 {
                let r = slider_rect(i, p[1]);
                if x >= r.left && x <= r.right && y >= r.top - 22.0 && y <= r.bottom + 22.0 {
                    let t = ((x - r.left) / (r.right - r.left)).clamp(0.0, 1.0);
                    let mut s = self.settings.write();
                    match i {
                        0 => s.duration = 0.08 + 0.52 * t as f64,
                        1 => s.dock_mag = 1.2 + 1.4 * t as f64,
                        _ => s.sensitivity = (120.0 + 280.0 * t) as i32,
                    }
                    drop(s);
                    self.persist("Control updated.");
                    return;
                }
            }
            let my = p[1].top + 337.0;
            if y >= my && y <= my + 34.0 && x >= p[1].left + 123.0 && x <= p[1].left + 300.0 {
                let mut s = self.settings.write();
                s.hotkey_mod = if x < p[1].left + 214.0 { "alt" } else { "ctrl" }.into();
                crate::input::keyboard_hook::configure(&s.hotkey_mod, s.hotkey_key);
                drop(s);
                self.persist("Switcher chord updated.");
                return;
            }
            let cell = (p[1].right - p[1].left - 40.0) / 3.0;
            for (i, (_, key)) in HOTKEYS.iter().enumerate() {
                let col = i % 3;
                let row = i / 3;
                let left = p[1].left + 18.0 + col as f32 * (cell + 2.0);
                let top = p[1].top + 352.0 + row as f32 * 30.0;
                let r = D2D_RECT_F {
                    left,
                    top,
                    right: left + cell - 5.0,
                    bottom: top + 26.0,
                };
                if inside(r, x, y) {
                    let mut s = self.settings.write();
                    s.hotkey_key = *key;
                    crate::input::keyboard_hook::configure(&s.hotkey_mod, s.hotkey_key);
                    drop(s);
                    self.persist("Switcher key updated.");
                    return;
                }
            }
            let py = p[1].top + 416.0;
            if y >= py - 2.0 && y <= py + 22.0 && x >= p[1].right - 128.0 && x <= p[1].right - 24.0
            {
                self.settings.write().pinch_enabled = x < p[1].right - 76.0;
                self.persist("Pinch gesture updated.");
                return;
            }
            if y >= p[1].top + 444.0 && y <= p[1].top + 478.0 {
                self.capture_pinch = true;
                crate::input::keyboard_hook::set_enabled(false);
                self.status = "Press the shortcut keys. Esc cancels.".into();
                unsafe {
                    let _ = SetFocus(self.hwnd);
                }
                return;
            }
            for (i, (_, rgb)) in ACCENTS.iter().enumerate() {
                let r = D2D_RECT_F {
                    left: p[1].left + 194.0 + i as f32 * 21.0,
                    top: p[1].top + 14.0,
                    right: p[1].left + 211.0 + i as f32 * 21.0,
                    bottom: p[1].top + 31.0,
                };
                if inside(r, x, y) {
                    self.settings.write().accent = *rgb;
                    self.persist("Accent color updated.");
                    return;
                }
            }
            return;
        }
        if inside(p[2], x, y) {
            if y < p[2].top + 54.0 && x > p[2].right - 70.0 {
                self.refresh_windows();
                self.status = "Window list refreshed.".into();
                return;
            }
            if y >= p[2].bottom - 45.0 && self.open_titles.len() > 10 {
                if x < (p[2].left + p[2].right) * 0.5 {
                    self.window_page = self.window_page.saturating_sub(1);
                } else {
                    self.window_page =
                        (self.window_page + 1).min((self.open_titles.len() - 1) / 10);
                }
                return;
            }
            if y >= p[2].top + 72.0 {
                let i = self.window_page * 10
                    + ((y - p[2].top - 72.0) / 37.0).floor().max(0.0) as usize;
                if i < self.open_titles.len() {
                    let title = self.open_titles[i].clone();
                    let mut s = self.settings.write();
                    if s.blocklist.contains(&title) {
                        s.blocklist.retain(|t| t != &title);
                    } else {
                        s.blocklist.push(title);
                    }
                    drop(s);
                    self.persist("Window exclusion updated.");
                    self.refresh_windows();
                }
            }
        }
    }
}

impl SettingsDialog {
    #[allow(dead_code)]
    fn render_legacy(&self) {
        self.d2d.begin_draw();
        let w = self.width as f32;
        let h = self.height as f32;
        let cfg = self.settings.read();
        let accent = D2D1_COLOR_F {
            r: cfg.accent[0] as f32 / 255.0,
            g: cfg.accent[1] as f32 / 255.0,
            b: cfg.accent[2] as f32 / 255.0,
            a: 1.0,
        };
        let white = D2D1_COLOR_F {
            r: 0.94,
            g: 0.96,
            b: 1.0,
            a: 1.0,
        };
        let muted = D2D1_COLOR_F {
            r: 0.64,
            g: 0.70,
            b: 0.80,
            a: 1.0,
        };
        let dim = D2D1_COLOR_F {
            r: 0.40,
            g: 0.47,
            b: 0.59,
            a: 1.0,
        };
        self.d2d.clear(0.025, 0.035, 0.055, 1.0);
        self.d2d.draw_glow(
            D2D_RECT_F {
                left: 50.0,
                top: 15.0,
                right: 400.0,
                bottom: 165.0,
            },
            90.0,
            D2D1_COLOR_F { a: 0.20, ..accent },
            8,
        );
        self.d2d.draw_text(
            "APP SWITCHER",
            D2D_RECT_F {
                left: 28.0,
                top: 18.0,
                right: 390.0,
                bottom: 50.0,
            },
            white,
            24.0,
            true,
            false,
        );
        self.d2d.draw_text(
            "A calmer, faster way to move between your windows",
            D2D_RECT_F {
                left: 30.0,
                top: 52.0,
                right: 730.0,
                bottom: 72.0,
            },
            muted,
            11.0,
            false,
            false,
        );
        if self.touchpad_warning && !cfg.warn_dismissed {
            let r = D2D_RECT_F {
                left: 24.0,
                top: 82.0,
                right: w - 24.0,
                bottom: 134.0,
            };
            draw_liquid_glass(&self.d2d, r, 14.0, accent, false);
            self.d2d.draw_text(
                "TOUCHPAD GESTURE NOTICE",
                D2D_RECT_F {
                    left: r.left + 16.0,
                    top: r.top + 8.0,
                    right: r.right - 330.0,
                    bottom: r.top + 28.0,
                },
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.80,
                    b: 0.40,
                    a: 1.0,
                },
                10.0,
                true,
                false,
            );
            self.d2d.draw_text(
                "Windows 3-finger swipe can conflict with AppSwitcher.",
                D2D_RECT_F {
                    left: r.left + 16.0,
                    top: r.top + 28.0,
                    right: r.right - 330.0,
                    bottom: r.bottom - 6.0,
                },
                muted,
                10.0,
                false,
                false,
            );
            self.d2d.draw_text(
                "TOUCHPAD SETTINGS",
                D2D_RECT_F {
                    left: r.right - 314.0,
                    top: r.top + 12.0,
                    right: r.right - 160.0,
                    bottom: r.bottom - 10.0,
                },
                white,
                9.0,
                true,
                true,
            );
            self.d2d.draw_text(
                "DISMISS",
                D2D_RECT_F {
                    left: r.right - 144.0,
                    top: r.top + 12.0,
                    right: r.right - 16.0,
                    bottom: r.bottom - 10.0,
                },
                accent,
                9.0,
                true,
                true,
            );
        }
        let mut p = panels(self.width);
        for q in &mut p {
            q.top = panel_y(&cfg);
            q.bottom = h - 102.0;
            draw_liquid_glass(&self.d2d, *q, 18.0, accent, false);
        }
        // LOOK: animations and layouts are selectable from the same view.
        self.d2d.draw_text(
            "LOOK",
            D2D_RECT_F {
                left: p[0].left + 17.0,
                top: p[0].top + 15.0,
                right: p[0].right - 16.0,
                bottom: p[0].top + 38.0,
            },
            accent,
            10.0,
            true,
            false,
        );
        self.d2d.draw_text(
            "Switch animation",
            D2D_RECT_F {
                left: p[0].left + 17.0,
                top: p[0].top + 45.0,
                right: p[0].right - 16.0,
                bottom: p[0].top + 67.0,
            },
            white,
            13.0,
            true,
            false,
        );
        for (i, (name, label)) in ANIMATIONS.iter().enumerate() {
            let r = animation_rect(i, p[0]);
            let active = cfg.animation == *name;
            draw_liquid_glass(&self.d2d, r, 9.0, accent, active);
            self.d2d.draw_text(
                label,
                r,
                if active { accent } else { muted },
                9.0,
                active,
                true,
            );
        }
        self.d2d.draw_text(
            "Picker layout",
            D2D_RECT_F {
                left: p[0].left + 17.0,
                top: p[0].top + 169.0,
                right: p[0].right - 16.0,
                bottom: p[0].top + 191.0,
            },
            white,
            13.0,
            true,
            false,
        );
        for (i, (name, label)) in LAYOUTS.iter().enumerate() {
            let r = layout_rect(i, p[0]);
            let active = cfg.layout == *name;
            draw_liquid_glass(&self.d2d, r, 10.0, accent, active);
            self.d2d.draw_text(
                label,
                D2D_RECT_F {
                    left: r.left + 12.0,
                    top: r.top + 5.0,
                    right: r.right - 40.0,
                    bottom: r.bottom - 5.0,
                },
                if active { white } else { muted },
                10.5,
                active,
                false,
            );
            self.d2d.draw_text(
                if active { "●" } else { "○" },
                D2D_RECT_F {
                    left: r.right - 32.0,
                    top: r.top + 4.0,
                    right: r.right - 10.0,
                    bottom: r.bottom - 4.0,
                },
                if active { accent } else { dim },
                10.0,
                true,
                true,
            );
        }
        let gap = 8.0;
        let half = (p[0].right - p[0].left - 40.0 - gap) * 0.5;
        let prev_layout = D2D_RECT_F {
            left: p[0].left + 16.0,
            top: p[0].bottom - 47.0,
            right: p[0].left + 16.0 + half,
            bottom: p[0].bottom - 12.0,
        };
        let prev_motion = D2D_RECT_F {
            left: prev_layout.right + gap,
            top: prev_layout.top,
            right: p[0].right - 16.0,
            bottom: prev_layout.bottom,
        };
        draw_liquid_glass(&self.d2d, prev_layout, 10.0, accent, true);
        self.d2d
            .draw_text("PREVIEW LAYOUT", prev_layout, white, 8.5, true, true);
        draw_liquid_glass(&self.d2d, prev_motion, 10.0, accent, false);
        self.d2d
            .draw_text("PREVIEW MOTION", prev_motion, white, 8.5, true, true);

        // FEEL: direct sliders, hotkey controls, pinch setup, and accent swatches.
        self.d2d.draw_text(
            "FEEL & SHORTCUTS",
            D2D_RECT_F {
                left: p[1].left + 17.0,
                top: p[1].top + 15.0,
                right: p[1].left + 190.0,
                bottom: p[1].top + 38.0,
            },
            accent,
            10.0,
            true,
            false,
        );
        for (i, title) in ["Animation speed", "Dock magnification", "Swipe sensitivity"]
            .iter()
            .enumerate()
        {
            let y = p[1].top + 48.0 + i as f32 * 82.0;
            let value = match i {
                0 => format!("{} ms", (cfg.duration * 1000.0).round() as i32),
                1 => format!("{:.2}×", cfg.dock_mag),
                _ => format!("{} px", cfg.sensitivity),
            };
            self.d2d.draw_text(
                title,
                D2D_RECT_F {
                    left: p[1].left + 18.0,
                    top: y,
                    right: p[1].right - 108.0,
                    bottom: y + 20.0,
                },
                white,
                11.5,
                false,
                false,
            );
            self.d2d.draw_text(
                &value,
                D2D_RECT_F {
                    left: p[1].right - 108.0,
                    top: y,
                    right: p[1].right - 18.0,
                    bottom: y + 20.0,
                },
                accent,
                11.0,
                true,
                true,
            );
            let r = slider_rect(i, p[1]);
            self.d2d.fill_rounded_rect(
                r,
                4.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.12,
                },
            );
            let t = match i {
                0 => ((cfg.duration - 0.08) / 0.52).clamp(0.0, 1.0) as f32,
                1 => ((cfg.dock_mag - 1.2) / 1.4).clamp(0.0, 1.0) as f32,
                _ => ((cfg.sensitivity as f64 - 120.0) / 280.0).clamp(0.0, 1.0) as f32,
            };
            let cx = r.left + (r.right - r.left) * t;
            self.d2d
                .fill_rounded_rect(D2D_RECT_F { right: cx, ..r }, 4.0, accent);
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: cx - 6.0,
                    top: r.top - 4.0,
                    right: cx + 6.0,
                    bottom: r.bottom + 4.0,
                },
                6.0,
                white,
            );
        }
        self.d2d.draw_text(
            "SWITCH WITH",
            D2D_RECT_F {
                left: p[1].left + 18.0,
                top: p[1].top + 295.0,
                right: p[1].left + 126.0,
                bottom: p[1].top + 319.0,
            },
            muted,
            9.0,
            true,
            false,
        );
        for (i, (label, on)) in [
            ("ALT", !cfg.hotkey_mod.eq_ignore_ascii_case("ctrl")),
            ("CTRL", cfg.hotkey_mod.eq_ignore_ascii_case("ctrl")),
        ]
        .iter()
        .enumerate()
        {
            let r = D2D_RECT_F {
                left: p[1].left + 123.0 + i as f32 * 91.0,
                top: p[1].top + 292.0,
                right: p[1].left + 209.0 + i as f32 * 91.0,
                bottom: p[1].top + 324.0,
            };
            draw_liquid_glass(&self.d2d, r, 9.0, accent, *on);
            self.d2d
                .draw_text(label, r, if *on { accent } else { muted }, 9.0, *on, true);
        }
        self.d2d.draw_text(
            &format!("KEY   {}", key_name(cfg.hotkey_key)),
            D2D_RECT_F {
                left: p[1].left + 18.0,
                top: p[1].top + 327.0,
                right: p[1].right - 18.0,
                bottom: p[1].top + 348.0,
            },
            white,
            10.0,
            true,
            false,
        );
        for (i, (label, _)) in HOTKEYS.iter().enumerate() {
            let col = i % 3;
            let row = i / 3;
            let cell = (p[1].right - p[1].left - 40.0) / 3.0;
            let left = p[1].left + 18.0 + col as f32 * (cell + 2.0);
            let top = p[1].top + 352.0 + row as f32 * 30.0;
            let r = D2D_RECT_F {
                left,
                top,
                right: left + cell - 5.0,
                bottom: top + 26.0,
            };
            let active = key_name(cfg.hotkey_key) == *label;
            draw_liquid_glass(&self.d2d, r, 8.0, accent, active);
            self.d2d.draw_text(
                label,
                r,
                if active { accent } else { muted },
                9.0,
                active,
                true,
            );
        }
        let py = p[1].top + 416.0;
        self.d2d.draw_text(
            "THREE-FINGER PINCH",
            D2D_RECT_F {
                left: p[1].left + 18.0,
                top: py,
                right: p[1].right - 150.0,
                bottom: py + 20.0,
            },
            white,
            9.5,
            true,
            false,
        );
        for (i, label) in ["ON", "OFF"].iter().enumerate() {
            let r = D2D_RECT_F {
                left: p[1].right - 128.0 + i as f32 * 54.0,
                top: py - 2.0,
                right: p[1].right - 78.0 + i as f32 * 54.0,
                bottom: py + 22.0,
            };
            let active = cfg.pinch_enabled == (i == 0);
            draw_liquid_glass(&self.d2d, r, 8.0, accent, active);
            self.d2d.draw_text(
                label,
                r,
                if active { accent } else { muted },
                8.5,
                active,
                true,
            );
        }
        let cap = D2D_RECT_F {
            left: p[1].left + 16.0,
            top: p[1].top + 444.0,
            right: p[1].right - 16.0,
            bottom: p[1].top + 478.0,
        };
        draw_liquid_glass(&self.d2d, cap, 9.0, accent, self.capture_pinch);
        self.d2d.draw_text(
            if self.capture_pinch {
                "PRESS SHORTCUT KEYS…"
            } else {
                "CAPTURE PINCH SHORTCUT"
            },
            cap,
            if self.capture_pinch { accent } else { muted },
            8.5,
            true,
            true,
        );
        for (i, (_, rgb)) in ACCENTS.iter().enumerate() {
            let r = D2D_RECT_F {
                left: p[1].left + 194.0 + i as f32 * 21.0,
                top: p[1].top + 14.0,
                right: p[1].left + 211.0 + i as f32 * 21.0,
                bottom: p[1].top + 31.0,
            };
            let c = D2D1_COLOR_F {
                r: rgb[0] as f32 / 255.0,
                g: rgb[1] as f32 / 255.0,
                b: rgb[2] as f32 / 255.0,
                a: 1.0,
            };
            self.d2d.fill_rounded_rect(r, 9.0, c);
            if cfg.accent == *rgb {
                self.d2d.draw_rounded_rect(r, 9.0, white, 2.0);
            }
        }

        // WINDOWS: title list mirrors the Python settings window with an exclusion toggle.
        self.d2d.draw_text(
            "HIDE FROM SWITCHER",
            D2D_RECT_F {
                left: p[2].left + 17.0,
                top: p[2].top + 15.0,
                right: p[2].right - 85.0,
                bottom: p[2].top + 37.0,
            },
            accent,
            10.0,
            true,
            false,
        );
        let refresh = D2D_RECT_F {
            left: p[2].right - 64.0,
            top: p[2].top + 12.0,
            right: p[2].right - 15.0,
            bottom: p[2].top + 40.0,
        };
        draw_liquid_glass(&self.d2d, refresh, 9.0, accent, false);
        self.d2d.draw_text("↻", refresh, white, 14.0, true, true);
        self.d2d.draw_text(
            &format!(
                "{} windows · click a row to exclude",
                self.open_titles.len()
            ),
            D2D_RECT_F {
                left: p[2].left + 17.0,
                top: p[2].top + 45.0,
                right: p[2].right - 14.0,
                bottom: p[2].top + 66.0,
            },
            muted,
            9.0,
            false,
            false,
        );
        let start = self.window_page * 10;
        let end = (start + 10).min(self.open_titles.len());
        for (row, title) in self
            .open_titles
            .get(start..end)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            let top = p[2].top + 72.0 + row as f32 * 37.0;
            let r = D2D_RECT_F {
                left: p[2].left + 12.0,
                top,
                right: p[2].right - 12.0,
                bottom: top + 32.0,
            };
            let blocked = cfg.blocklist.contains(title);
            draw_liquid_glass(&self.d2d, r, 8.0, accent, blocked);
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: r.left + 9.0,
                    top: r.top + 11.0,
                    right: r.left + 17.0,
                    bottom: r.top + 19.0,
                },
                4.0,
                if blocked { accent } else { dim },
            );
            let name = if title.chars().count() > 31 {
                format!("{}…", title.chars().take(30).collect::<String>())
            } else {
                title.clone()
            };
            self.d2d.draw_text(
                &name,
                D2D_RECT_F {
                    left: r.left + 25.0,
                    top: r.top + 4.0,
                    right: r.right - 48.0,
                    bottom: r.bottom - 4.0,
                },
                if blocked { muted } else { white },
                9.5,
                blocked,
                false,
            );
            self.d2d.draw_text(
                if blocked { "HIDE" } else { "SHOW" },
                D2D_RECT_F {
                    left: r.right - 46.0,
                    top: r.top + 4.0,
                    right: r.right - 7.0,
                    bottom: r.bottom - 4.0,
                },
                if blocked { accent } else { dim },
                8.0,
                true,
                true,
            );
        }
        if self.open_titles.is_empty() {
            self.d2d.draw_text(
                "No eligible windows found",
                D2D_RECT_F {
                    left: p[2].left + 24.0,
                    top: p[2].top + 110.0,
                    right: p[2].right - 24.0,
                    bottom: p[2].top + 150.0,
                },
                muted,
                12.0,
                false,
                true,
            );
        }
        let pages = (self.open_titles.len() + 9) / 10;
        if pages > 1 {
            self.d2d.draw_text(
                &format!("PAGE {} / {}      ‹      ›", self.window_page + 1, pages),
                D2D_RECT_F {
                    left: p[2].left + 16.0,
                    top: p[2].bottom - 45.0,
                    right: p[2].right - 16.0,
                    bottom: p[2].bottom - 15.0,
                },
                muted,
                9.0,
                true,
                true,
            );
        }
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: 0.0,
                top: h - 88.0,
                right: w,
                bottom: h,
            },
            D2D1_COLOR_F {
                r: 0.035,
                g: 0.047,
                b: 0.07,
                a: 1.0,
            },
        );
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: 0.0,
                top: h - 88.0,
                right: w,
                bottom: h - 87.0,
            },
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.10,
            },
        );
        self.d2d.draw_text(
            &self.status,
            D2D_RECT_F {
                left: 26.0,
                top: h - 59.0,
                right: 340.0,
                bottom: h - 33.0,
            },
            muted,
            9.0,
            false,
            false,
        );
        for (i, label, on) in [
            (0, "Replace system Alt+Tab", cfg.alt_tab),
            (1, "Start with Windows", self.autostart),
        ] {
            let x = 360.0 + i as f32 * 250.0;
            self.d2d.draw_text(
                label,
                D2D_RECT_F {
                    left: x,
                    top: h - 59.0,
                    right: x + 190.0,
                    bottom: h - 31.0,
                },
                white,
                9.5,
                false,
                false,
            );
            let r = D2D_RECT_F {
                left: x + 194.0,
                top: h - 58.0,
                right: x + 232.0,
                bottom: h - 34.0,
            };
            self.d2d.fill_rounded_rect(
                r,
                12.0,
                if on {
                    accent
                } else {
                    D2D1_COLOR_F {
                        r: 0.20,
                        g: 0.23,
                        b: 0.29,
                        a: 1.0,
                    }
                },
            );
            let cx = if on { r.right - 12.0 } else { r.left + 12.0 };
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: cx - 8.0,
                    top: r.top + 4.0,
                    right: cx + 8.0,
                    bottom: r.bottom - 4.0,
                },
                8.0,
                white,
            );
        }
        let save = D2D_RECT_F {
            left: w - 150.0,
            top: h - 62.0,
            right: w - 24.0,
            bottom: h - 22.0,
        };
        self.d2d.fill_rounded_rect(save, 11.0, accent);
        self.d2d.draw_text(
            "SAVE",
            save,
            D2D1_COLOR_F {
                r: 0.035,
                g: 0.047,
                b: 0.07,
                a: 1.0,
            },
            9.5,
            true,
            true,
        );
        let _ = self.d2d.end_draw();
    }
}

fn shortcut_text(mods: &[u32], vk: u32) -> String {
    let mut v: Vec<String> = mods
        .iter()
        .map(|m| {
            match *m {
                0x11 => "Ctrl",
                0x12 => "Alt",
                0x10 => "Shift",
                0x5b => "Win",
                _ => "?",
            }
            .into()
        })
        .collect();
    if vk != 0 {
        v.push(
            HOTKEYS
                .iter()
                .find(|(_, k)| *k == vk)
                .map(|(n, _)| *n)
                .unwrap_or("Key")
                .into(),
        );
    }
    if v.is_empty() {
        "No shortcut".into()
    } else {
        v.join(" + ")
    }
}
impl SettingsDialog {
    fn handle_click(&mut self, x: f32, y: f32) {
        let c = self.settings.read().clone();
        let warn = self.touchpad_warning && !c.warn_dismissed;
        let p = dashboard_panels(self.width, self.height, warn);
        if warn && (82.0..=140.0).contains(&y) {
            if x > self.width as f32 - 170.0 {
                self.settings.write().warn_dismissed = true;
                self.persist("Gesture notice dismissed.");
            } else if x > self.width as f32 - 330.0 {
                unsafe {
                    let _ = ShellExecuteW(
                        self.hwnd,
                        w!("open"),
                        w!("ms-settings:devices-touchpad"),
                        None,
                        None,
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                    );
                }
            }
            return;
        }
        let fy = self.height as f32 - 86.0;
        if y >= fy {
            if (30.0..=320.0).contains(&x) {
                let mut s = self.settings.write();
                s.alt_tab = !s.alt_tab;
                let on = s.alt_tab;
                drop(s);
                crate::input::keyboard_hook::set_enabled(on);
                self.persist(if on {
                    "Alt+Tab replacement enabled."
                } else {
                    "Alt+Tab replacement disabled."
                });
            } else if (350.0..=605.0).contains(&x) {
                let on = !self.autostart;
                if set_autostart(on) {
                    self.autostart = on;
                    self.status = if on {
                        "Will start with Windows.".into()
                    } else {
                        "Windows startup disabled.".into()
                    };
                } else {
                    self.status = "Could not change startup setting.".into();
                }
            } else if inside(save_button(self.width, self.height), x, y) {
                self.persist("All settings saved.");
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            return;
        }
        if let Some(open) = self.open_combo {
            let anchor = match open {
                0 => look_combo(p[0], 0),
                1 => look_combo(p[0], 1),
                2 => hotkey_combo(p[0], 0),
                _ => hotkey_combo(p[0], 1),
            };
            let count = match open {
                0 => ANIMATIONS.len(),
                1 => LAYOUTS.len(),
                2 => 2,
                _ => HOTKEYS.len(),
            };
            let menu = D2D_RECT_F {
                left: anchor.left,
                top: anchor.bottom + 4.0,
                right: anchor.right,
                bottom: anchor.bottom + 4.0 + count as f32 * 32.0,
            };
            if inside(menu, x, y) {
                let index = ((y - menu.top) / 32.0).floor() as usize;
                let mut s = self.settings.write();
                match open {
                    0 if index < ANIMATIONS.len() => s.animation = ANIMATIONS[index].0.into(),
                    1 if index < LAYOUTS.len() => s.layout = LAYOUTS[index].0.into(),
                    2 if index < 2 => s.hotkey_mod = if index == 0 { "alt" } else { "ctrl" }.into(),
                    3 if index < HOTKEYS.len() => s.hotkey_key = HOTKEYS[index].1,
                    _ => {}
                }
                if open >= 2 {
                    crate::input::keyboard_hook::configure(&s.hotkey_mod, s.hotkey_key);
                }
                drop(s);
                self.open_combo = None;
                self.persist("Setting updated.");
                return;
            }
            self.open_combo = None;
            return;
        }
        if inside(p[0], x, y) {
            for i in 0..2 {
                if inside(look_combo(p[0], i), x, y) {
                    self.open_combo = Some(i);
                    return;
                }
            }
            for i in 0..2 {
                if inside(hotkey_combo(p[0], i), x, y) {
                    self.open_combo = Some(i + 2);
                    return;
                }
            }
            let sw = D2D_RECT_F {
                left: p[0].right - 70.0,
                top: p[0].top + 285.0,
                right: p[0].right - 20.0,
                bottom: p[0].top + 315.0,
            };
            if inside(sw, x, y) {
                self.settings.write().pinch_enabled = !c.pinch_enabled;
                self.persist("Pinch gesture updated.");
                return;
            }
            if inside(pinch_capture_rect(p[0]), x, y) {
                self.capture_pinch = true;
                crate::input::keyboard_hook::set_enabled(false);
                self.status = "Press shortcut keys. Esc cancels.".into();
                unsafe {
                    let _ = SetFocus(self.hwnd);
                }
                return;
            }
            if inside(pinch_clear_rect(p[0]), x, y) {
                let mut s = self.settings.write();
                s.pinch_mods.clear();
                s.pinch_vk = 0;
                drop(s);
                self.persist("Pinch shortcut cleared.");
                return;
            }
            for i in 0..2 {
                if inside(preview_button(p[0], i), x, y) {
                    let _ = self.event_tx.send(if i == 0 {
                        AppEvent::PreviewPicker
                    } else {
                        AppEvent::PreviewAnimation
                    });
                    self.notify_main();
                    return;
                }
            }
        }
        if inside(p[1], x, y) {
            for i in 0..3 {
                let r = slider_track(p[1], i);
                if x >= r.left && x <= r.right && y >= r.top - 20.0 && y <= r.bottom + 20.0 {
                    let t = ((x - r.left) / (r.right - r.left)).clamp(0.0, 1.0);
                    let mut s = self.settings.write();
                    match i {
                        0 => {
                            s.duration =
                                (((80.0 + 520.0 * t) / 10.0).round() * 10.0) as f64 / 1000.0
                        }
                        1 => s.dock_mag = ((1.2 + 1.4 * t) * 20.0).round() as f64 / 20.0,
                        _ => s.sensitivity = ((120.0 + 280.0 * t) / 5.0).round() as i32 * 5,
                    }
                    drop(s);
                    self.persist("Setting updated.");
                    return;
                }
            }
            for (i, (_, rgb)) in ACCENTS.iter().enumerate() {
                if inside(accent_swatch(p[1], i), x, y) {
                    self.settings.write().accent = *rgb;
                    self.persist("Accent updated.");
                    return;
                }
            }
        }
        if inside(p[2], x, y) {
            if y < p[2].top + 49.0 && x > p[2].right - 74.0 {
                self.refresh_windows();
                return;
            }
            let pages = (self.open_titles.len() + 7) / 8;
            if pages > 1 && y >= p[2].bottom - 34.0 {
                self.window_page = if x < (p[2].left + p[2].right) * 0.5 {
                    self.window_page.saturating_sub(1)
                } else {
                    (self.window_page + 1).min(pages - 1)
                };
                return;
            }
            for i in 0..8 {
                if inside(window_row(p[2], i), x, y) {
                    let j = self.window_page * 8 + i;
                    if let Some(t) = self.open_titles.get(j).cloned() {
                        let mut s = self.settings.write();
                        if s.blocklist.contains(&t) {
                            s.blocklist.retain(|v| v != &t);
                        } else {
                            s.blocklist.push(t);
                        }
                        drop(s);
                        self.persist("Window exclusion updated.");
                        self.refresh_windows();
                    }
                    return;
                }
            }
        }
    }
    fn hovered(&self, r: D2D_RECT_F) -> bool {
        self.mouse.is_some_and(|(x, y)| inside(r, x, y))
    }

    /// Raised surface for a settings section.
    fn card(&self, r: D2D_RECT_F) {
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                top: r.top + 2.0,
                bottom: r.bottom + 2.0,
                ..r
            },
            9.0,
            rgba(0.0, 0.0, 0.0, 0.22),
        );
        self.d2d.fill_rounded_rect(r, 8.0, theme::CARD);
        self.d2d.draw_rounded_rect(r, 8.0, theme::CARD_STROKE, 1.0);
    }

    /// Fluent-style control surface (combo box, text field, button).
    fn control(&self, r: D2D_RECT_F, active: bool, ac: D2D1_COLOR_F) {
        let fill = if active || self.hovered(r) {
            theme::CONTROL_HOVER
        } else {
            theme::CONTROL
        };
        self.d2d.fill_rounded_rect(r, 6.0, fill);
        self.d2d.draw_rounded_rect(
            r,
            6.0,
            if active { ac } else { theme::CONTROL_STROKE },
            1.0,
        );
        // Fluent inputs carry a slightly brighter bottom edge.
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: r.left + 6.0,
                top: r.bottom - 1.0,
                right: r.right - 6.0,
                bottom: r.bottom,
            },
            if active { ac } else { rgba(1.0, 1.0, 1.0, 0.07) },
        );
    }

    fn section_header(&self, glyph: char, title: &str, r: D2D_RECT_F) {
        self.d2d.icon(
            glyph,
            D2D_RECT_F {
                right: r.left + 18.0,
                ..r
            },
            theme::TEXT2,
            15.0,
        );
        self.d2d.text(
            title,
            D2D_RECT_F {
                left: r.left + 28.0,
                ..r
            },
            theme::TEXT1,
            14.0,
            600,
            Font::Text,
            Align::Leading,
        );
    }

    /// Windows 11 toggle switch, centred inside the existing hit rect.
    fn toggle(&self, hit: D2D_RECT_F, on: bool, ac: D2D1_COLOR_F) {
        let cx = (hit.left + hit.right) * 0.5;
        let cy = (hit.top + hit.bottom) * 0.5;
        let track = D2D_RECT_F {
            left: cx - 20.0,
            top: cy - 10.0,
            right: cx + 20.0,
            bottom: cy + 10.0,
        };
        let hover = self.hovered(hit);
        if on {
            self.d2d.fill_rounded_rect(track, 10.0, ac);
            self.d2d
                .fill_circle(track.right - 10.0, cy, if hover { 7.0 } else { 6.0 }, on_accent(ac));
        } else {
            self.d2d.fill_rounded_rect(
                track,
                10.0,
                if hover { rgba(1.0, 1.0, 1.0, 0.06) } else { rgba(0.0, 0.0, 0.0, 0.10) },
            );
            self.d2d
                .draw_rounded_rect(track, 10.0, rgba(1.0, 1.0, 1.0, 0.55), 1.0);
            self.d2d.fill_circle(
                track.left + 10.0,
                cy,
                if hover { 6.0 } else { 5.0 },
                rgba(1.0, 1.0, 1.0, 0.78),
            );
        }
    }

    fn button(&self, r: D2D_RECT_F, label: &str, glyph: Option<char>, ac: D2D1_COLOR_F) {
        self.control(r, false, ac);
        let size = 12.5;
        let text_w = self.d2d.measure(label, size, 400, Font::Text);
        let icon_w = if glyph.is_some() { 22.0 } else { 0.0 };
        let left = (r.left + r.right - text_w - icon_w) * 0.5;
        if let Some(g) = glyph {
            self.d2d.icon(
                g,
                D2D_RECT_F {
                    left,
                    right: left + 14.0,
                    ..r
                },
                theme::TEXT2,
                12.0,
            );
        }
        self.d2d.text(
            label,
            D2D_RECT_F {
                left: left + icon_w,
                right: r.right,
                ..r
            },
            theme::TEXT1,
            size,
            400,
            Font::Text,
            Align::Leading,
        );
    }

    /// Draw a shortcut such as "Ctrl + Tab" as keycaps centred in `r`.
    fn keycaps(&self, chord: &str, r: D2D_RECT_F) {
        let keys: Vec<&str> = chord.split(" + ").filter(|k| !k.is_empty()).collect();
        if keys.is_empty() {
            self.d2d.text(
                "Not set",
                r,
                theme::TEXT3,
                12.5,
                400,
                Font::Text,
                Align::Center,
            );
            return;
        }
        let size = 12.0;
        let pad = 9.0;
        let plus = 18.0;
        let widths: Vec<f32> = keys
            .iter()
            .map(|k| self.d2d.measure(k, size, 600, Font::Text) + pad * 2.0)
            .collect();
        let total: f32 = widths.iter().sum::<f32>() + plus * (keys.len() - 1) as f32;
        let mut x = (r.left + r.right - total) * 0.5;
        let cy = (r.top + r.bottom) * 0.5;
        for (i, (key, w)) in keys.iter().zip(&widths).enumerate() {
            if i > 0 {
                self.d2d.text(
                    "+",
                    D2D_RECT_F {
                        left: x,
                        top: r.top,
                        right: x + plus,
                        bottom: r.bottom,
                    },
                    theme::TEXT3,
                    12.0,
                    400,
                    Font::Text,
                    Align::Center,
                );
                x += plus;
            }
            let cap = D2D_RECT_F {
                left: x,
                top: cy - 12.0,
                right: x + w,
                bottom: cy + 12.0,
            };
            self.d2d.fill_rounded_rect(cap, 5.0, rgba(1.0, 1.0, 1.0, 0.08));
            self.d2d.draw_rounded_rect(cap, 5.0, rgba(1.0, 1.0, 1.0, 0.10), 1.0);
            self.d2d.fill_rect(
                D2D_RECT_F {
                    left: cap.left + 4.0,
                    top: cap.bottom - 1.5,
                    right: cap.right - 4.0,
                    bottom: cap.bottom,
                },
                rgba(0.0, 0.0, 0.0, 0.35),
            );
            self.d2d
                .text(key, cap, theme::TEXT1, size, 600, Font::Text, Align::Center);
            x += w;
        }
    }

    pub fn render(&self) {
        self.d2d.begin_draw();
        let w = self.width as f32;
        let h = self.height as f32;
        let c = self.settings.read();
        let ac = rgba(
            c.accent[0] as f32 / 255.0,
            c.accent[1] as f32 / 255.0,
            c.accent[2] as f32 / 255.0,
            1.0,
        );

        // ── Window background & header ───────────────────────────────────
        self.d2d.clear(
            theme::BACKGROUND.r,
            theme::BACKGROUND.g,
            theme::BACKGROUND.b,
            1.0,
        );
        let tile = D2D_RECT_F {
            left: 30.0,
            top: 27.0,
            right: 64.0,
            bottom: 61.0,
        };
        self.d2d.fill_rounded_rect(tile, 8.0, D2D1_COLOR_F { a: 0.16, ..ac });
        self.d2d
            .draw_rounded_rect(tile, 8.0, D2D1_COLOR_F { a: 0.28, ..ac }, 1.0);
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 41.0,
                top: 41.0,
                right: 57.0,
                bottom: 53.0,
            },
            3.0,
            rgba(1.0, 1.0, 1.0, 0.38),
        );
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 37.0,
                top: 35.0,
                right: 53.0,
                bottom: 47.0,
            },
            3.0,
            ac,
        );
        let title = D2D_RECT_F {
            left: 76.0,
            top: 27.0,
            right: 340.0,
            bottom: 61.0,
        };
        self.d2d
            .text("AppSwitcher", title, theme::TEXT1, 20.0, 600, Font::Display, Align::Leading);
        let title_w = self.d2d.measure("AppSwitcher", 20.0, 600, Font::Display);
        self.d2d.text(
            "Settings",
            D2D_RECT_F {
                left: title.left + title_w + 10.0,
                ..title
            },
            theme::TEXT2,
            20.0,
            400,
            Font::Display,
            Align::Leading,
        );
        let close = D2D_RECT_F {
            left: w - 58.0,
            top: 27.0,
            right: w - 26.0,
            bottom: 59.0,
        };
        if self.hovered(close) {
            self.d2d.fill_rounded_rect(close, 6.0, rgba(1.0, 1.0, 1.0, 0.07));
        }
        self.d2d.icon(glyph::CLOSE, close, theme::TEXT2, 11.0);

        // ── Touchpad warning (Fluent InfoBar) ────────────────────────────
        let warn = self.touchpad_warning && !c.warn_dismissed;
        if warn {
            let r = D2D_RECT_F {
                left: 30.0,
                top: 78.0,
                right: w - 30.0,
                bottom: 138.0,
            };
            self.d2d.fill_rounded_rect(r, 8.0, rgba(0.26, 0.21, 0.11, 1.0));
            self.d2d.draw_rounded_rect(r, 8.0, rgba(1.0, 0.85, 0.45, 0.16), 1.0);
            let amber = rgba(0.99, 0.82, 0.38, 1.0);
            self.d2d.icon(
                glyph::WARNING,
                D2D_RECT_F {
                    left: r.left + 16.0,
                    top: r.top,
                    right: r.left + 40.0,
                    bottom: r.bottom,
                },
                amber,
                16.0,
            );
            self.d2d.text(
                "Windows three-finger swipes are still on",
                D2D_RECT_F {
                    left: r.left + 52.0,
                    top: r.top + 9.0,
                    right: r.right - 330.0,
                    bottom: r.top + 31.0,
                },
                theme::TEXT1,
                13.0,
                600,
                Font::Text,
                Align::Leading,
            );
            self.d2d.text(
                "Set Touchpad › Three-finger gestures › Swipes to Nothing so AppSwitcher receives them.",
                D2D_RECT_F {
                    left: r.left + 52.0,
                    top: r.top + 30.0,
                    right: r.right - 330.0,
                    bottom: r.bottom - 8.0,
                },
                theme::TEXT2,
                12.0,
                400,
                Font::Text,
                Align::Leading,
            );
            self.button(
                D2D_RECT_F {
                    left: r.right - 290.0,
                    top: r.top + 12.0,
                    right: r.right - 154.0,
                    bottom: r.bottom - 12.0,
                },
                "Open Touchpad",
                None,
                ac,
            );
            let dismiss = D2D_RECT_F {
                left: r.right - 142.0,
                top: r.top + 12.0,
                right: r.right - 18.0,
                bottom: r.bottom - 12.0,
            };
            if self.hovered(dismiss) {
                self.d2d.fill_rounded_rect(dismiss, 6.0, rgba(1.0, 1.0, 1.0, 0.06));
            }
            self.d2d
                .text("Dismiss", dismiss, theme::TEXT2, 12.5, 400, Font::Text, Align::Center);
        }

        let p = dashboard_panels(self.width, self.height, warn);
        for r in p {
            self.card(r);
        }

        // ── Look ─────────────────────────────────────────────────────────
        let a = p[0];
        self.section_header(
            glyph::PERSONALIZE,
            "Look",
            D2D_RECT_F {
                left: a.left + 20.0,
                top: a.top + 14.0,
                right: a.right - 18.0,
                bottom: a.top + 40.0,
            },
        );
        for (t, y) in [
            ("Switch animation", a.top + 48.0),
            ("Picker layout", a.top + 126.0),
            ("Switch shortcut", a.top + 204.0),
        ] {
            self.d2d.text(
                t,
                D2D_RECT_F {
                    left: a.left + 20.0,
                    top: y,
                    right: a.right - 20.0,
                    bottom: y + 20.0,
                },
                theme::TEXT2,
                12.5,
                400,
                Font::Text,
                Align::Leading,
            );
        }
        let anim = ANIMATIONS
            .iter()
            .find(|(k, _)| *k == c.animation)
            .map(|(_, v)| *v)
            .unwrap_or("Slide");
        let layout = LAYOUTS
            .iter()
            .find(|(k, _)| *k == c.layout)
            .map(|(_, v)| *v)
            .unwrap_or("Dock");
        for (i, v) in [anim, layout].iter().enumerate() {
            let r = look_combo(a, i);
            let open = self.open_combo == Some(i);
            self.control(r, open, ac);
            self.d2d.text(
                v,
                D2D_RECT_F {
                    left: r.left + 12.0,
                    right: r.right - 40.0,
                    ..r
                },
                theme::TEXT1,
                13.0,
                400,
                Font::Text,
                Align::Leading,
            );
            self.d2d.icon(
                if open { glyph::CHEVRON_UP } else { glyph::CHEVRON_DOWN },
                D2D_RECT_F {
                    left: r.right - 36.0,
                    right: r.right - 12.0,
                    ..r
                },
                theme::TEXT2,
                10.0,
            );
        }
        let mod_label = if c.hotkey_mod.eq_ignore_ascii_case("ctrl") {
            "Ctrl"
        } else {
            "Alt"
        };
        for (i, v) in [mod_label, key_name(c.hotkey_key)].iter().enumerate() {
            let r = hotkey_combo(a, i);
            let open = self.open_combo == Some(2 + i);
            self.control(r, open, ac);
            self.d2d.text(
                v,
                D2D_RECT_F {
                    left: r.left + 12.0,
                    right: r.right - 34.0,
                    ..r
                },
                theme::TEXT1,
                13.0,
                600,
                Font::Text,
                Align::Leading,
            );
            self.d2d.icon(
                if open { glyph::CHEVRON_UP } else { glyph::CHEVRON_DOWN },
                D2D_RECT_F {
                    left: r.right - 32.0,
                    right: r.right - 10.0,
                    ..r
                },
                theme::TEXT2,
                10.0,
            );
        }
        self.d2d.text(
            "+",
            D2D_RECT_F {
                left: hotkey_combo(a, 0).right,
                top: hotkey_combo(a, 0).top,
                right: hotkey_combo(a, 1).left,
                bottom: hotkey_combo(a, 0).bottom,
            },
            theme::TEXT3,
            13.0,
            400,
            Font::Text,
            Align::Center,
        );
        self.d2d.text(
            "Pinch shortcut (three fingers)",
            D2D_RECT_F {
                left: a.left + 20.0,
                top: a.top + 285.0,
                right: a.right - 90.0,
                bottom: a.top + 315.0,
            },
            theme::TEXT2,
            12.5,
            400,
            Font::Text,
            Align::Leading,
        );
        self.toggle(
            D2D_RECT_F {
                left: a.right - 70.0,
                top: a.top + 285.0,
                right: a.right - 20.0,
                bottom: a.top + 315.0,
            },
            c.pinch_enabled,
            ac,
        );
        let cap = pinch_capture_rect(a);
        self.control(cap, self.capture_pinch, ac);
        if self.capture_pinch {
            self.d2d.text(
                "Press a shortcut…  (Esc to cancel)",
                cap,
                ac,
                12.5,
                400,
                Font::Text,
                Align::Center,
            );
        } else {
            self.d2d.icon(
                glyph::KEYBOARD,
                D2D_RECT_F {
                    left: cap.left + 12.0,
                    right: cap.left + 32.0,
                    ..cap
                },
                theme::TEXT3,
                13.0,
            );
            self.keycaps(&shortcut_text(&c.pinch_mods, c.pinch_vk), cap);
        }
        self.button(pinch_clear_rect(a), "Clear", None, ac);
        self.button(preview_button(a, 0), "Preview layout", Some(glyph::VIEW_ALL), ac);
        self.button(preview_button(a, 1), "Preview animation", Some(glyph::PLAY), ac);

        // ── Feel ─────────────────────────────────────────────────────────
        let m = p[1];
        self.section_header(
            glyph::SPEED,
            "Feel",
            D2D_RECT_F {
                left: m.left + 20.0,
                top: m.top + 14.0,
                right: m.right - 20.0,
                bottom: m.top + 40.0,
            },
        );
        let labels = ["Animation duration", "Dock magnification", "Swipe sensitivity"];
        let vals = [
            format!("{} ms", (c.duration * 1000.0).round() as i32),
            format!("{:.2}×", c.dock_mag),
            format!("{} px", c.sensitivity),
        ];
        for i in 0..3 {
            let y = m.top + 47.0 + i as f32 * 82.0;
            let r = slider_track(m, i);
            self.d2d.text(
                labels[i],
                D2D_RECT_F {
                    left: m.left + 20.0,
                    top: y,
                    right: m.right - 120.0,
                    bottom: y + 20.0,
                },
                theme::TEXT1,
                13.0,
                400,
                Font::Text,
                Align::Leading,
            );
            self.d2d.text(
                &vals[i],
                D2D_RECT_F {
                    left: m.right - 140.0,
                    top: y,
                    right: r.right,
                    bottom: y + 20.0,
                },
                theme::TEXT2,
                12.5,
                400,
                Font::Text,
                Align::Trailing,
            );
            let t = match i {
                0 => ((c.duration * 1000.0 - 80.0) / 520.0).clamp(0.0, 1.0) as f32,
                1 => ((c.dock_mag - 1.2) / 1.4).clamp(0.0, 1.0) as f32,
                _ => ((c.sensitivity as f64 - 120.0) / 280.0).clamp(0.0, 1.0) as f32,
            };
            let cy = (r.top + r.bottom) * 0.5;
            let track = D2D_RECT_F {
                left: r.left,
                top: cy - 2.0,
                right: r.right,
                bottom: cy + 2.0,
            };
            let cx = track.left + (track.right - track.left) * t;
            self.d2d.fill_rounded_rect(track, 2.0, rgba(1.0, 1.0, 1.0, 0.16));
            self.d2d
                .fill_rounded_rect(D2D_RECT_F { right: cx, ..track }, 2.0, ac);
            // Win11 thumb: neutral disc with an accent core that grows on hover.
            let hover = self.hovered(D2D_RECT_F {
                top: r.top - 20.0,
                bottom: r.bottom + 20.0,
                ..r
            });
            self.d2d.fill_circle(cx, cy, 10.0, theme::THUMB);
            self.d2d.draw_rounded_rect(
                D2D_RECT_F {
                    left: cx - 10.0,
                    top: cy - 10.0,
                    right: cx + 10.0,
                    bottom: cy + 10.0,
                },
                10.0,
                rgba(1.0, 1.0, 1.0, 0.10),
                1.0,
            );
            self.d2d.fill_circle(cx, cy, if hover { 7.0 } else { 5.5 }, ac);
        }
        self.d2d.text(
            "Accent",
            D2D_RECT_F {
                left: m.left + 20.0,
                top: m.bottom - 43.0,
                right: m.left + 94.0,
                bottom: m.bottom - 19.0,
            },
            theme::TEXT2,
            12.5,
            400,
            Font::Text,
            Align::Leading,
        );
        for (i, (_, rgb)) in ACCENTS.iter().enumerate() {
            let r = accent_swatch(m, i);
            let z = rgba(
                rgb[0] as f32 / 255.0,
                rgb[1] as f32 / 255.0,
                rgb[2] as f32 / 255.0,
                1.0,
            );
            let cx = (r.left + r.right) * 0.5;
            let cy = (r.top + r.bottom) * 0.5;
            let selected = c.accent == *rgb;
            if selected {
                self.d2d.draw_rounded_rect(
                    D2D_RECT_F {
                        left: cx - 15.0,
                        top: cy - 15.0,
                        right: cx + 15.0,
                        bottom: cy + 15.0,
                    },
                    15.0,
                    theme::TEXT1,
                    1.5,
                );
            }
            let radius = if self.hovered(r) && !selected { 12.0 } else { 11.0 };
            self.d2d.fill_circle(cx, cy, radius, z);
            if selected {
                self.d2d.icon(glyph::CHECK, r, on_accent(z), 11.0);
            }
        }

        // ── Hide from switcher ───────────────────────────────────────────
        let q = p[2];
        self.section_header(
            glyph::HIDE,
            "Hide from switcher",
            D2D_RECT_F {
                left: q.left + 18.0,
                top: q.top + 12.0,
                right: q.right - 100.0,
                bottom: q.top + 38.0,
            },
        );
        let count = format!("{} open", self.open_titles.len());
        let count_w = self.d2d.measure(&count, 11.5, 400, Font::Text) + 16.0;
        let pill = D2D_RECT_F {
            left: q.right - 46.0 - count_w,
            top: q.top + 15.0,
            right: q.right - 46.0,
            bottom: q.top + 35.0,
        };
        self.d2d.fill_rounded_rect(pill, 10.0, rgba(1.0, 1.0, 1.0, 0.06));
        self.d2d
            .text(&count, pill, theme::TEXT2, 11.5, 400, Font::Text, Align::Center);
        let rr = D2D_RECT_F {
            left: q.right - 40.0,
            top: q.top + 11.0,
            right: q.right - 12.0,
            bottom: q.top + 39.0,
        };
        if self.hovered(rr) {
            self.d2d.fill_rounded_rect(rr, 6.0, rgba(1.0, 1.0, 1.0, 0.07));
        }
        self.d2d.icon(glyph::REFRESH, rr, theme::TEXT2, 13.0);
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: q.left + 1.0,
                top: q.top + 50.0,
                right: q.right - 1.0,
                bottom: q.top + 51.0,
            },
            theme::CARD_STROKE,
        );
        let start = self.window_page * 8;
        for i in 0..8 {
            let Some(t) = self.open_titles.get(start + i) else {
                break;
            };
            let r = window_row(q, i);
            let hidden = c.blocklist.contains(t);
            if self.hovered(r) {
                self.d2d.fill_rounded_rect(r, 6.0, rgba(1.0, 1.0, 1.0, 0.045));
            }
            self.d2d.icon(
                if hidden { glyph::HIDE } else { glyph::VIEW },
                D2D_RECT_F {
                    left: r.left + 6.0,
                    right: r.left + 30.0,
                    ..r
                },
                if hidden { ac } else { theme::TEXT3 },
                13.0,
            );
            self.d2d.text(
                t,
                D2D_RECT_F {
                    left: r.left + 38.0,
                    right: r.right - 68.0,
                    ..r
                },
                if hidden { theme::TEXT3 } else { theme::TEXT1 },
                12.5,
                400,
                Font::Text,
                Align::Leading,
            );
            self.toggle(
                D2D_RECT_F {
                    left: r.right - 60.0,
                    top: r.top + 6.0,
                    right: r.right - 8.0,
                    bottom: r.bottom - 6.0,
                },
                hidden,
                ac,
            );
        }
        let pages = self.open_titles.len().div_ceil(8);
        if pages > 1 {
            let bar = D2D_RECT_F {
                left: q.left + 12.0,
                top: q.bottom - 34.0,
                right: q.right - 12.0,
                bottom: q.bottom - 8.0,
            };
            let prev_on = self.window_page > 0;
            let next_on = self.window_page + 1 < pages;
            self.d2d.icon(
                glyph::CHEVRON_LEFT,
                D2D_RECT_F {
                    right: bar.left + 28.0,
                    ..bar
                },
                if prev_on { theme::TEXT1 } else { theme::TEXT3 },
                11.0,
            );
            self.d2d.icon(
                glyph::CHEVRON_RIGHT,
                D2D_RECT_F {
                    left: bar.right - 28.0,
                    ..bar
                },
                if next_on { theme::TEXT1 } else { theme::TEXT3 },
                11.0,
            );
            self.d2d.text(
                &format!("Page {} of {}", self.window_page + 1, pages),
                bar,
                theme::TEXT2,
                12.0,
                400,
                Font::Text,
                Align::Center,
            );
        }
        if self.open_titles.is_empty() {
            self.d2d.text(
                "No windows to show right now",
                D2D_RECT_F {
                    left: q.left + 25.0,
                    top: q.top + 110.0,
                    right: q.right - 25.0,
                    bottom: q.top + 145.0,
                },
                theme::TEXT2,
                13.0,
                400,
                Font::Text,
                Align::Center,
            );
        }

        // ── Footer ───────────────────────────────────────────────────────
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: 0.0,
                top: h - 88.0,
                right: w,
                bottom: h,
            },
            theme::FOOTER,
        );
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: 0.0,
                top: h - 88.0,
                right: w,
                bottom: h - 87.0,
            },
            theme::CARD_STROKE,
        );
        for (label, left, right) in [
            ("Replace the Windows Alt+Tab switcher", 30.0, 258.0),
            ("Start with Windows", 355.0, 535.0),
        ] {
            self.d2d.text(
                label,
                D2D_RECT_F {
                    left,
                    top: h - 58.0,
                    right,
                    bottom: h - 28.0,
                },
                theme::TEXT1,
                13.0,
                400,
                Font::Text,
                Align::Leading,
            );
        }
        for (i, on) in [(0, c.alt_tab), (1, self.autostart)] {
            let mut r = footer_switch(i, self.width);
            r.top = h - 58.0;
            r.bottom = h - 28.0;
            self.toggle(r, on, ac);
        }
        let save = save_button(self.width, self.height);
        self.d2d.text(
            &self.status,
            D2D_RECT_F {
                left: 640.0,
                top: save.top,
                right: save.left - 20.0,
                bottom: save.bottom,
            },
            theme::TEXT3,
            12.0,
            400,
            Font::Text,
            Align::Trailing,
        );
        let save_fill = if self.hovered(save) {
            rgba(ac.r * 0.9, ac.g * 0.9, ac.b * 0.9, 1.0)
        } else {
            ac
        };
        self.d2d.fill_rounded_rect(save, 6.0, save_fill);
        self.d2d.draw_rounded_rect(save, 6.0, rgba(1.0, 1.0, 1.0, 0.12), 1.0);
        self.d2d
            .text("Save", save, on_accent(ac), 13.0, 600, Font::Text, Align::Center);

        // ── Open dropdown (Fluent flyout) ────────────────────────────────
        if let Some(open) = self.open_combo {
            let anchor = match open {
                0 => look_combo(a, 0),
                1 => look_combo(a, 1),
                2 => hotkey_combo(a, 0),
                _ => hotkey_combo(a, 1),
            };
            let items: Vec<String> = match open {
                0 => ANIMATIONS.iter().map(|(_, v)| v.to_string()).collect(),
                1 => LAYOUTS.iter().map(|(_, v)| v.to_string()).collect(),
                2 => vec!["Alt".into(), "Ctrl".into()],
                _ => HOTKEYS.iter().map(|(v, _)| v.to_string()).collect(),
            };
            let current = match open {
                0 => anim.to_string(),
                1 => layout.to_string(),
                2 => mod_label.to_string(),
                _ => key_name(c.hotkey_key).to_string(),
            };
            // Same geometry as the click handler: rows are 32px from menu.top.
            let menu = D2D_RECT_F {
                left: anchor.left,
                top: anchor.bottom + 4.0,
                right: anchor.right,
                bottom: anchor.bottom + 4.0 + items.len() as f32 * 32.0,
            };
            let outer = D2D_RECT_F {
                top: menu.top - 4.0,
                bottom: menu.bottom + 4.0,
                ..menu
            };
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: outer.left - 2.0,
                    top: outer.top + 2.0,
                    right: outer.right + 2.0,
                    bottom: outer.bottom + 8.0,
                },
                10.0,
                rgba(0.0, 0.0, 0.0, 0.35),
            );
            self.d2d.fill_rounded_rect(outer, 8.0, theme::FLYOUT);
            self.d2d.draw_rounded_rect(outer, 8.0, theme::CONTROL_STROKE, 1.0);
            for (i, item) in items.iter().enumerate() {
                let slot = D2D_RECT_F {
                    left: menu.left + 4.0,
                    top: menu.top + i as f32 * 32.0 + 2.0,
                    right: menu.right - 4.0,
                    bottom: menu.top + (i + 1) as f32 * 32.0 - 2.0,
                };
                let is_current = *item == current;
                if is_current || self.hovered(slot) {
                    self.d2d
                        .fill_rounded_rect(slot, 5.0, rgba(1.0, 1.0, 1.0, 0.06));
                }
                if is_current {
                    let cy = (slot.top + slot.bottom) * 0.5;
                    self.d2d.fill_rounded_rect(
                        D2D_RECT_F {
                            left: slot.left,
                            top: cy - 8.0,
                            right: slot.left + 3.0,
                            bottom: cy + 8.0,
                        },
                        1.5,
                        ac,
                    );
                }
                self.d2d.text(
                    item,
                    D2D_RECT_F {
                        left: slot.left + 12.0,
                        right: slot.right - 8.0,
                        ..slot
                    },
                    theme::TEXT1,
                    13.0,
                    if is_current { 600 } else { 400 },
                    Font::Text,
                    Align::Leading,
                );
            }
        }
        let _ = self.d2d.end_draw();
    }
}

const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// Text/glyph colour that stays readable on top of an accent fill.
fn on_accent(c: D2D1_COLOR_F) -> D2D1_COLOR_F {
    let luminance = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if luminance > 0.5 {
        rgba(0.06, 0.07, 0.09, 1.0)
    } else {
        rgba(1.0, 1.0, 1.0, 1.0)
    }
}

/// Windows 11 dark theme tokens for the settings window.
mod theme {
    use super::rgba;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

    pub const BACKGROUND: D2D1_COLOR_F = rgba(0.110, 0.114, 0.125, 1.0);
    pub const CARD: D2D1_COLOR_F = rgba(0.153, 0.157, 0.169, 1.0);
    pub const CARD_STROKE: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.065);
    pub const CONTROL: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.055);
    pub const CONTROL_HOVER: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.085);
    pub const CONTROL_STROKE: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.075);
    pub const FLYOUT: D2D1_COLOR_F = rgba(0.173, 0.176, 0.188, 1.0);
    pub const FOOTER: D2D1_COLOR_F = rgba(0.094, 0.098, 0.106, 1.0);
    pub const THUMB: D2D1_COLOR_F = rgba(0.27, 0.275, 0.29, 1.0);
    pub const TEXT1: D2D1_COLOR_F = rgba(0.96, 0.965, 0.975, 1.0);
    pub const TEXT2: D2D1_COLOR_F = rgba(0.73, 0.75, 0.79, 1.0);
    pub const TEXT3: D2D1_COLOR_F = rgba(0.50, 0.52, 0.56, 1.0);
}

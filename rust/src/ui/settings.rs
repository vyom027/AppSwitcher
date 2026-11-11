use crate::config::Settings;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::layouts::draw_liquid_glass;
use crate::render::d2d::D2DContext;
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
    BeginPaint, CreateRoundRectRgn, EndPaint, RedrawWindow, SetWindowRgn, PAINTSTRUCT,
    RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, SetFocus};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, LoadIconW, PostMessageW,
    RegisterClassExW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    HTCAPTION, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_CLOSE, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_NCLBUTTONDOWN, WM_PAINT, WM_SYSKEYDOWN, WNDCLASSEXW,
    WS_EX_APPWINDOW, WS_POPUP,
};
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
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, 48, 48);
            let _ = SetWindowRgn(hwnd, region, true);
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
    pub fn render(&self) {
        self.d2d.begin_draw();
        let w = self.width as f32;
        let h = self.height as f32;
        let c = self.settings.read();
        let ac = D2D1_COLOR_F {
            r: c.accent[0] as f32 / 255.0,
            g: c.accent[1] as f32 / 255.0,
            b: c.accent[2] as f32 / 255.0,
            a: 1.0,
        };
        let white = D2D1_COLOR_F {
            r: 0.94,
            g: 0.96,
            b: 1.0,
            a: 1.0,
        };
        let muted = D2D1_COLOR_F {
            r: 0.60,
            g: 0.65,
            b: 0.74,
            a: 1.0,
        };
        let edge = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.07,
        };
        let dark = D2D1_COLOR_F {
            r: 0.105,
            g: 0.12,
            b: 0.155,
            a: 0.97,
        };
        self.d2d.clear(0.045, 0.052, 0.073, 1.0);
        let body = D2D_RECT_F {
            left: 2.0,
            top: 2.0,
            right: w - 2.0,
            bottom: h - 2.0,
        };
        self.d2d.fill_rounded_rect(
            body,
            26.0,
            D2D1_COLOR_F {
                r: 0.075,
                g: 0.083,
                b: 0.11,
                a: 1.0,
            },
        );
        self.d2d.draw_glow(
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: 320.0,
                bottom: 110.0,
            },
            70.0,
            D2D1_COLOR_F { a: 0.13, ..ac },
            7,
        );
        self.d2d.draw_rounded_rect(body, 26.0, edge, 1.0);
        let icon = D2D_RECT_F {
            left: 30.0,
            top: 27.0,
            right: 64.0,
            bottom: 61.0,
        };
        self.d2d.fill_rounded_rect(
            icon,
            10.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.055,
            },
        );
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 37.0,
                top: 35.0,
                right: 52.0,
                bottom: 46.0,
            },
            3.0,
            ac,
        );
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 44.0,
                top: 43.0,
                right: 59.0,
                bottom: 54.0,
            },
            3.0,
            D2D1_COLOR_F {
                r: 0.58,
                g: 0.63,
                b: 0.72,
                a: 1.0,
            },
        );
        self.d2d.draw_text(
            "AppSwitcher",
            D2D_RECT_F {
                left: 76.0,
                top: 27.0,
                right: 340.0,
                bottom: 62.0,
            },
            white,
            22.0,
            true,
            false,
        );
        self.d2d.draw_text(
            "x",
            D2D_RECT_F {
                left: w - 58.0,
                top: 27.0,
                right: w - 26.0,
                bottom: 59.0,
            },
            muted,
            14.0,
            false,
            true,
        );
        let warn = self.touchpad_warning && !c.warn_dismissed;
        if warn {
            let r = D2D_RECT_F {
                left: 30.0,
                top: 78.0,
                right: w - 30.0,
                bottom: 138.0,
            };
            self.d2d.fill_rounded_rect(
                r,
                16.0,
                D2D1_COLOR_F {
                    r: 0.45,
                    g: 0.32,
                    b: 0.12,
                    a: 0.24,
                },
            );
            self.d2d.draw_rounded_rect(
                r,
                16.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.75,
                    b: 0.2,
                    a: 0.27,
                },
                1.0,
            );
            self.d2d.draw_text(
                "!",
                D2D_RECT_F {
                    left: r.left + 16.0,
                    top: r.top + 14.0,
                    right: r.left + 40.0,
                    bottom: r.bottom - 10.0,
                },
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.82,
                    b: 0.48,
                    a: 1.0,
                },
                17.0,
                true,
                true,
            );
            self.d2d.draw_text(
                "Windows 3-finger swipes are still on",
                D2D_RECT_F {
                    left: r.left + 52.0,
                    top: r.top + 8.0,
                    right: r.right - 330.0,
                    bottom: r.top + 30.0,
                },
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 0.82,
                    b: 0.48,
                    a: 1.0,
                },
                12.0,
                true,
                false,
            );
            self.d2d.draw_text(
                "Set Touchpad > Three-finger gestures > Swipes to Nothing.",
                D2D_RECT_F {
                    left: r.left + 52.0,
                    top: r.top + 31.0,
                    right: r.right - 330.0,
                    bottom: r.bottom - 7.0,
                },
                muted,
                10.0,
                false,
                false,
            );
            self.d2d.draw_text(
                "Open Touchpad",
                D2D_RECT_F {
                    left: r.right - 290.0,
                    top: r.top + 12.0,
                    right: r.right - 154.0,
                    bottom: r.bottom - 12.0,
                },
                white,
                10.0,
                true,
                true,
            );
            self.d2d.draw_text(
                "Dismiss",
                D2D_RECT_F {
                    left: r.right - 142.0,
                    top: r.top + 12.0,
                    right: r.right - 18.0,
                    bottom: r.bottom - 12.0,
                },
                muted,
                10.0,
                false,
                true,
            );
        }
        let p = dashboard_panels(self.width, self.height, warn);
        for r in p {
            draw_liquid_glass(&self.d2d, r, 20.0, ac, false);
        }
        let a = p[0];
        self.d2d.draw_text(
            "L O O K",
            D2D_RECT_F {
                left: a.left + 20.0,
                top: a.top + 17.0,
                right: a.right - 18.0,
                bottom: a.top + 38.0,
            },
            muted,
            10.0,
            true,
            false,
        );
        for (t, y) in [
            ("Switch animation", a.top + 48.0),
            ("Picker layout", a.top + 126.0),
            ("Switch shortcut", a.top + 204.0),
        ] {
            self.d2d.draw_text(
                t,
                D2D_RECT_F {
                    left: a.left + 20.0,
                    top: y,
                    right: a.right - 20.0,
                    bottom: y + 19.0,
                },
                muted,
                11.0,
                false,
                false,
            );
        }
        let anim = ANIMATIONS
            .iter()
            .find(|(k, _)| *k == c.animation)
            .map(|(_, v)| *v)
            .unwrap_or("slide");
        let layout = LAYOUTS
            .iter()
            .find(|(k, _)| *k == c.layout)
            .map(|(_, v)| *v)
            .unwrap_or("Dock");
        for (i, v) in [anim, layout].iter().enumerate() {
            let r = look_combo(a, i);
            self.d2d.fill_rounded_rect(r, 13.0, dark);
            self.d2d.draw_rounded_rect(r, 13.0, edge, 1.0);
            self.d2d.draw_text(v, r, white, 12.0, false, false);
            self.d2d.draw_text(
                "v",
                D2D_RECT_F {
                    left: r.right - 34.0,
                    top: r.top + 5.0,
                    right: r.right - 12.0,
                    bottom: r.bottom - 5.0,
                },
                muted,
                10.0,
                false,
                true,
            );
        }
        for (i, v) in [c.hotkey_mod.as_str(), key_name(c.hotkey_key)]
            .iter()
            .enumerate()
        {
            let r = hotkey_combo(a, i);
            self.d2d.fill_rounded_rect(r, 13.0, dark);
            self.d2d.draw_rounded_rect(r, 13.0, edge, 1.0);
            self.d2d.draw_text(v, r, white, 11.0, false, true);
            self.d2d.draw_text(
                "v",
                D2D_RECT_F {
                    left: r.right - 28.0,
                    top: r.top + 5.0,
                    right: r.right - 10.0,
                    bottom: r.bottom - 5.0,
                },
                muted,
                10.0,
                false,
                true,
            );
        }
        self.d2d.draw_text(
            "+",
            D2D_RECT_F {
                left: hotkey_combo(a, 0).right,
                top: a.top + 240.0,
                right: hotkey_combo(a, 1).left,
                bottom: a.top + 266.0,
            },
            muted,
            12.0,
            false,
            true,
        );
        self.d2d.draw_text(
            "Pinch shortcut (3-finger)",
            D2D_RECT_F {
                left: a.left + 20.0,
                top: a.top + 286.0,
                right: a.right - 90.0,
                bottom: a.top + 313.0,
            },
            muted,
            11.0,
            false,
            false,
        );
        let ps = D2D_RECT_F {
            left: a.right - 70.0,
            top: a.top + 285.0,
            right: a.right - 20.0,
            bottom: a.top + 315.0,
        };
        self.d2d.fill_rounded_rect(
            ps,
            15.0,
            if c.pinch_enabled {
                ac
            } else {
                D2D1_COLOR_F {
                    r: 0.22,
                    g: 0.24,
                    b: 0.29,
                    a: 1.0,
                }
            },
        );
        let k = if c.pinch_enabled {
            ps.right - 15.0
        } else {
            ps.left + 15.0
        };
        self.d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: k - 12.0,
                top: ps.top + 3.0,
                right: k + 12.0,
                bottom: ps.bottom - 3.0,
            },
            12.0,
            white,
        );
        let cap = pinch_capture_rect(a);
        self.d2d.fill_rounded_rect(cap, 13.0, dark);
        self.d2d
            .draw_rounded_rect(cap, 13.0, if self.capture_pinch { ac } else { edge }, 1.0);
        let chord = shortcut_text(&c.pinch_mods, c.pinch_vk);
        self.d2d.draw_text(
            if self.capture_pinch {
                "Press keys"
            } else {
                &chord
            },
            cap,
            if self.capture_pinch { ac } else { white },
            11.0,
            false,
            true,
        );
        let clear = pinch_clear_rect(a);
        self.d2d.fill_rounded_rect(clear, 13.0, dark);
        self.d2d.draw_rounded_rect(clear, 13.0, edge, 1.0);
        self.d2d.draw_text("Clear", clear, white, 10.0, false, true);
        for i in 0..2 {
            let r = preview_button(a, i);
            self.d2d.fill_rounded_rect(r, 13.0, dark);
            self.d2d.draw_rounded_rect(r, 13.0, edge, 1.0);
            self.d2d.draw_text(
                if i == 0 {
                    "Preview layout"
                } else {
                    "Preview anim"
                },
                r,
                white,
                10.0,
                false,
                true,
            );
        }
        let m = p[1];
        self.d2d.draw_text(
            "F E E L",
            D2D_RECT_F {
                left: m.left + 20.0,
                top: m.top + 17.0,
                right: m.right - 20.0,
                bottom: m.top + 38.0,
            },
            muted,
            10.0,
            true,
            false,
        );
        let labels = ["Animation speed", "Dock magnify", "Sensitivity"];
        let vals = [
            format!("{} ms", (c.duration * 1000.0).round() as i32),
            format!("{:.2}x", c.dock_mag),
            format!("{}", c.sensitivity),
        ];
        for i in 0..3 {
            let y = m.top + 47.0 + i as f32 * 82.0;
            self.d2d.draw_text(
                labels[i],
                D2D_RECT_F {
                    left: m.left + 20.0,
                    top: y,
                    right: m.right - 95.0,
                    bottom: y + 20.0,
                },
                muted,
                11.0,
                false,
                false,
            );
            self.d2d.draw_text(
                &vals[i],
                D2D_RECT_F {
                    left: m.right - 96.0,
                    top: y,
                    right: m.right - 20.0,
                    bottom: y + 20.0,
                },
                ac,
                11.0,
                true,
                true,
            );
            let r = slider_track(m, i);
            self.d2d.fill_rounded_rect(
                r,
                3.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.12,
                },
            );
            let t = match i {
                0 => ((c.duration * 1000.0 - 80.0) / 520.0).clamp(0.0, 1.0) as f32,
                1 => ((c.dock_mag - 1.2) / 1.4).clamp(0.0, 1.0) as f32,
                _ => ((c.sensitivity as f64 - 120.0) / 280.0).clamp(0.0, 1.0) as f32,
            };
            let cx = r.left + (r.right - r.left) * t;
            self.d2d
                .fill_rounded_rect(D2D_RECT_F { right: cx, ..r }, 3.0, ac);
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: cx - 8.0,
                    top: r.top - 7.0,
                    right: cx + 8.0,
                    bottom: r.bottom + 7.0,
                },
                8.0,
                white,
            );
            self.d2d.draw_rounded_rect(
                D2D_RECT_F {
                    left: cx - 8.0,
                    top: r.top - 7.0,
                    right: cx + 8.0,
                    bottom: r.bottom + 7.0,
                },
                8.0,
                ac,
                1.3,
            );
        }
        self.d2d.draw_text(
            "A C C E N T",
            D2D_RECT_F {
                left: m.left + 20.0,
                top: m.bottom - 43.0,
                right: m.left + 94.0,
                bottom: m.bottom - 18.0,
            },
            muted,
            9.0,
            true,
            false,
        );
        for (i, (_, rgb)) in ACCENTS.iter().enumerate() {
            let r = accent_swatch(m, i);
            let z = D2D1_COLOR_F {
                r: rgb[0] as f32 / 255.0,
                g: rgb[1] as f32 / 255.0,
                b: rgb[2] as f32 / 255.0,
                a: 1.0,
            };
            self.d2d.fill_rounded_rect(r, 12.0, z);
            if c.accent == *rgb {
                self.d2d.draw_rounded_rect(
                    D2D_RECT_F {
                        left: r.left - 2.0,
                        top: r.top - 2.0,
                        right: r.right + 2.0,
                        bottom: r.bottom + 2.0,
                    },
                    14.0,
                    white,
                    2.0,
                );
            }
        }
        let q = p[2];
        self.d2d.draw_text(
            "H I D E   F R O M   S W I T C H E R",
            D2D_RECT_F {
                left: q.left + 18.0,
                top: q.top + 15.0,
                right: q.right - 92.0,
                bottom: q.top + 37.0,
            },
            muted,
            9.0,
            true,
            false,
        );
        self.d2d.draw_text(
            &format!("{} · live", self.open_titles.len()),
            D2D_RECT_F {
                left: q.right - 94.0,
                top: q.top + 15.0,
                right: q.right - 44.0,
                bottom: q.top + 36.0,
            },
            muted,
            9.0,
            false,
            true,
        );
        let rr = D2D_RECT_F {
            left: q.right - 39.0,
            top: q.top + 12.0,
            right: q.right - 14.0,
            bottom: q.top + 37.0,
        };
        self.d2d.fill_rounded_rect(
            rr,
            13.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.05,
            },
        );
        self.d2d.draw_text("R", rr, muted, 10.0, true, true);
        let start = self.window_page * 8;
        for i in 0..8 {
            if let Some(t) = self.open_titles.get(start + i) {
                let r = window_row(q, i);
                let on = c.blocklist.contains(t);
                let dot = D2D_RECT_F {
                    left: r.left + 12.0,
                    top: r.top + 15.0,
                    right: r.left + 20.0,
                    bottom: r.top + 23.0,
                };
                self.d2d.fill_rounded_rect(
                    dot,
                    4.0,
                    if on {
                        ac
                    } else {
                        D2D1_COLOR_F { a: 0.2, ..white }
                    },
                );
                let name = if t.chars().count() > 34 {
                    format!("{}…", t.chars().take(33).collect::<String>())
                } else {
                    t.clone()
                };
                self.d2d.draw_text(
                    &name,
                    D2D_RECT_F {
                        left: r.left + 31.0,
                        top: r.top + 6.0,
                        right: r.right - 68.0,
                        bottom: r.bottom - 6.0,
                    },
                    if on { muted } else { white },
                    10.5,
                    false,
                    false,
                );
                let sr = D2D_RECT_F {
                    left: r.right - 60.0,
                    top: r.top + 6.0,
                    right: r.right - 8.0,
                    bottom: r.bottom - 6.0,
                };
                self.d2d.fill_rounded_rect(
                    sr,
                    14.0,
                    if on {
                        ac
                    } else {
                        D2D1_COLOR_F {
                            r: 0.21,
                            g: 0.23,
                            b: 0.28,
                            a: 1.0,
                        }
                    },
                );
                let cx = if on { sr.right - 14.0 } else { sr.left + 14.0 };
                self.d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: cx - 11.0,
                        top: sr.top + 3.0,
                        right: cx + 11.0,
                        bottom: sr.bottom - 3.0,
                    },
                    11.0,
                    white,
                );
            }
        }
        if self.open_titles.is_empty() {
            self.d2d.draw_text(
                "No eligible windows found",
                D2D_RECT_F {
                    left: q.left + 25.0,
                    top: q.top + 110.0,
                    right: q.right - 25.0,
                    bottom: q.top + 145.0,
                },
                muted,
                12.0,
                false,
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
                r: 0.07,
                g: 0.078,
                b: 0.102,
                a: 1.0,
            },
        );
        self.d2d.fill_rect(
            D2D_RECT_F {
                left: 30.0,
                top: h - 88.0,
                right: w - 30.0,
                bottom: h - 87.0,
            },
            edge,
        );
        self.d2d.draw_text(
            "Replace system switcher",
            D2D_RECT_F {
                left: 30.0,
                top: h - 60.0,
                right: 258.0,
                bottom: h - 30.0,
            },
            white,
            10.5,
            false,
            false,
        );
        self.d2d.draw_text(
            "Start with Windows",
            D2D_RECT_F {
                left: 355.0,
                top: h - 60.0,
                right: 535.0,
                bottom: h - 30.0,
            },
            white,
            10.5,
            false,
            false,
        );
        for (i, on) in [(0, c.alt_tab), (1, self.autostart)] {
            let mut r = footer_switch(i, self.width);
            r.top = h - 58.0;
            r.bottom = h - 28.0;
            self.d2d.fill_rounded_rect(
                r,
                15.0,
                if on {
                    ac
                } else {
                    D2D1_COLOR_F {
                        r: 0.22,
                        g: 0.24,
                        b: 0.29,
                        a: 1.0,
                    }
                },
            );
            let cx = if on { r.right - 15.0 } else { r.left + 15.0 };
            self.d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: cx - 12.0,
                    top: r.top + 3.0,
                    right: cx + 12.0,
                    bottom: r.bottom - 3.0,
                },
                12.0,
                white,
            );
        }
        let save = save_button(self.width, self.height);
        self.d2d.fill_rounded_rect(save, 13.0, ac);
        self.d2d.draw_text(
            "Save",
            save,
            D2D1_COLOR_F {
                r: 0.05,
                g: 0.06,
                b: 0.08,
                a: 1.0,
            },
            11.0,
            true,
            true,
        );
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
                2 => vec!["alt".into(), "ctrl".into()],
                _ => HOTKEYS.iter().map(|(v, _)| v.to_string()).collect(),
            };
            let current = match open {
                0 => anim.to_string(),
                1 => layout.to_string(),
                2 => c.hotkey_mod.clone(),
                _ => key_name(c.hotkey_key).to_string(),
            };
            let menu = D2D_RECT_F {
                left: anchor.left,
                top: anchor.bottom + 4.0,
                right: anchor.right,
                bottom: anchor.bottom + 4.0 + items.len() as f32 * 32.0,
            };
            self.d2d.fill_rounded_rect(
                menu,
                12.0,
                D2D1_COLOR_F {
                    r: 0.10,
                    g: 0.115,
                    b: 0.15,
                    a: 1.0,
                },
            );
            self.d2d.draw_rounded_rect(menu, 12.0, edge, 1.0);
            for (i, item) in items.iter().enumerate() {
                let row = D2D_RECT_F {
                    left: menu.left + 5.0,
                    top: menu.top + 4.0 + i as f32 * 32.0,
                    right: menu.right - 5.0,
                    bottom: menu.top + 4.0 + (i + 1) as f32 * 32.0,
                };
                if *item == current {
                    self.d2d
                        .fill_rounded_rect(row, 8.0, D2D1_COLOR_F { a: 0.12, ..ac });
                }
                self.d2d.draw_text(
                    item,
                    row,
                    if *item == current { ac } else { white },
                    10.5,
                    *item == current,
                    false,
                );
            }
        }
        let _ = self.d2d.end_draw();
    }
}

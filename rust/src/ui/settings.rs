//! Settings window: a sidebar of pages with Fluent-style setting rows.
//!
//! Rendering is immediate-mode: every frame records the rectangle of each
//! interactive element in `hits`, and mouse handling looks clicks up in that
//! same list, so hit areas can never drift away from what is drawn.

use crate::config::Settings;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::render::d2d::{glyph, Align, D2DContext, Font};
use crossbeam_channel::Sender;
use std::cell::{Cell, RefCell};
use std::mem::size_of;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Arc;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE,
    TRACKMOUSEEVENT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowLongPtrW, IsWindowVisible, KillTimer,
    LoadCursorW, LoadIconW, PostMessageW, RegisterClassExW, SendMessageW, SetCursor,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
    HTCAPTION, HTCLIENT, HWND_TOP, IDC_ARROW, IDC_HAND, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW,
    SW_SHOWNORMAL, WM_CLOSE, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NCLBUTTONDOWN, WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER,
    WNDCLASSEXW, WS_EX_APPWINDOW, WS_POPUP,
};

/// Sent after TrackMouseEvent(TME_LEAVE) when the cursor leaves the window.
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Timer that animates the live previews while Settings is visible.
const PREVIEW_TIMER: usize = 1;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const WINDOW_W: i32 = 1080;
const WINDOW_H: i32 = 700;
const SIDEBAR_W: f32 = 248.0;
const CONTENT_PAD: f32 = 40.0;
const ROW_H: f32 = 68.0;
const ROW_GAP: f32 = 8.0;
const WINDOWS_PER_PAGE: usize = 8;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Appearance,
    Motion,
    Gestures,
    Keyboard,
    Hidden,
    General,
}

const PAGES: &[(Page, char, &str, &str)] = &[
    (Page::Appearance, glyph::PERSONALIZE, "Appearance", "Choose how switching looks."),
    (Page::Motion, glyph::SPEED, "Motion", "Tune the speed and feel of every switch."),
    (Page::Gestures, glyph::TOUCH, "Gestures", "Three-finger touchpad gestures."),
    (Page::Keyboard, glyph::KEYBOARD, "Keyboard", "Alt+Tab replacement and shortcuts."),
    (
        Page::Hidden,
        glyph::HIDE,
        "Hidden windows",
        "Windows switched on here never appear in the switcher.",
    ),
    (Page::General, glyph::SETTINGS, "General", "Startup, files and version."),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Combo {
    Animation,
    Layout,
    HotkeyMod,
    HotkeyKey,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slider {
    Duration,
    Magnification,
    Sensitivity,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Toggle {
    AltTab,
    Autostart,
    Pinch,
    FollowWindowsAnimations,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Hit {
    Nav(Page),
    Close,
    Combo(Combo),
    ComboItem(Combo, usize),
    /// Click-away area behind an open dropdown.
    Dismiss,
    Slider(Slider),
    Toggle(Toggle),
    Accent(usize),
    PinchRecord,
    PinchClear,
    PreviewPicker,
    PreviewAnimation,
    HideWindow(usize),
    PagePrev,
    PageNext,
    Refresh,
    WarnOpen,
    WarnDismiss,
    OpenConfigFolder,
}

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

fn set_autostart(on: bool) -> bool {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if on {
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let value = format!("\"{}\"", exe.display());
        Command::new("reg.exe")
            .creation_flags(CREATE_NO_WINDOW)
            .args(["add", key, "/v", "AppSwitcher", "/t", "REG_SZ", "/d", &value, "/f"])
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

/// True only when the "AppSwitcher" startup entry launches *this* executable.
/// An entry left behind by the Python version (or another copy of the EXE)
/// must not make the switch look on, since it would start a different app.
fn autostart_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let exe = exe.display().to_string().to_lowercase();
    Command::new("reg.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "AppSwitcher",
        ])
        .output()
        .map(|o| {
            o.status.success() && String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&exe)
        })
        .unwrap_or(false)
}

fn key_name(vk: u32) -> &'static str {
    HOTKEYS
        .iter()
        .find(|(_, code)| *code == vk)
        .map(|(name, _)| *name)
        .unwrap_or("Tab")
}

fn shortcut_text(mods: &[u32], vk: u32) -> String {
    let mut parts: Vec<String> = mods
        .iter()
        .map(|m| {
            match *m {
                0x11 => "Ctrl",
                0x12 => "Alt",
                0x10 => "Shift",
                0x5b => "Win",
                _ => "?",
            }
            .to_string()
        })
        .collect();
    if vk != 0 {
        parts.push(
            HOTKEYS
                .iter()
                .find(|(_, k)| *k == vk)
                .map(|(n, _)| n.to_string())
                .unwrap_or_else(|| {
                    char::from_u32(vk)
                        .filter(|c| c.is_ascii_alphanumeric())
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| format!("Key {vk:#04x}"))
                }),
        );
    }
    parts.join(" + ")
}

fn inside(r: D2D_RECT_F, x: f32, y: f32) -> bool {
    x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
}

fn rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

fn inset(r: D2D_RECT_F, dx: f32, dy: f32) -> D2D_RECT_F {
    rect(r.left + dx, r.top + dy, r.right - dx, r.bottom - dy)
}

const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

fn with_alpha(c: D2D1_COLOR_F, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a, ..c }
}

fn lighten(c: D2D1_COLOR_F, amount: f32) -> D2D1_COLOR_F {
    rgba(
        (c.r + amount).min(1.0),
        (c.g + amount).min(1.0),
        (c.b + amount).min(1.0),
        c.a,
    )
}

/// Text/glyph colour that stays readable on top of an accent fill.
fn on_accent(c: D2D1_COLOR_F) -> D2D1_COLOR_F {
    let luminance = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if luminance > 0.5 {
        rgba(0.05, 0.05, 0.07, 1.0)
    } else {
        rgba(1.0, 1.0, 1.0, 1.0)
    }
}

/// Neutral "zinc" palette; the user's accent provides the colour.
mod theme {
    use super::rgba;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

    pub const BACKGROUND: D2D1_COLOR_F = rgba(0.063, 0.063, 0.075, 1.0);
    pub const SIDEBAR: D2D1_COLOR_F = rgba(0.043, 0.043, 0.051, 1.0);
    pub const CARD: D2D1_COLOR_F = rgba(0.094, 0.094, 0.110, 1.0);
    pub const CARD_HOVER: D2D1_COLOR_F = rgba(0.110, 0.110, 0.128, 1.0);
    pub const STROKE: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.065);
    pub const CONTROL: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.055);
    pub const CONTROL_HOVER: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.09);
    pub const CONTROL_STROKE: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.09);
    pub const FLYOUT: D2D1_COLOR_F = rgba(0.125, 0.125, 0.145, 1.0);
    pub const TEXT1: D2D1_COLOR_F = rgba(0.957, 0.957, 0.965, 1.0);
    pub const TEXT2: D2D1_COLOR_F = rgba(0.631, 0.631, 0.667, 1.0);
    pub const TEXT3: D2D1_COLOR_F = rgba(0.443, 0.443, 0.478, 1.0);
    pub const GREEN: D2D1_COLOR_F = rgba(0.29, 0.85, 0.50, 1.0);
    pub const AMBER: D2D1_COLOR_F = rgba(0.99, 0.78, 0.33, 1.0);
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
    page: Page,
    open_combo: Option<Combo>,
    dragging: Option<Slider>,
    /// Cursor position in client coordinates while over the window.
    mouse: Option<(f32, f32)>,
    /// Clock for the live previews.
    epoch: std::time::Instant,
    /// Interactive regions recorded by the last render, in draw order.
    hits: RefCell<Vec<(D2D_RECT_F, Hit)>>,
    /// Slider tracks recorded by the last render (for dragging).
    tracks: RefCell<Vec<(Slider, D2D_RECT_F)>>,
    /// Anchor rect of the open dropdown, recorded while drawing it.
    combo_anchor: Cell<Option<D2D_RECT_F>>,
}
unsafe impl Send for SettingsDialog {}
unsafe impl Sync for SettingsDialog {}

fn mouse_xy(l: LPARAM) -> (f32, f32) {
    (
        (l.0 as usize & 0xffff) as i16 as f32,
        ((l.0 as usize >> 16) & 0xffff) as i16 as f32,
    )
}

unsafe extern "system" fn settings_wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if ptr == 0 {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let dlg = &mut *(ptr as *mut SettingsDialog);
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let _ = BeginPaint(hwnd, &mut ps);
            dlg.render();
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_TIMER if w.0 == PREVIEW_TIMER => {
            if IsWindowVisible(hwnd).as_bool() {
                dlg.render();
            } else {
                let _ = KillTimer(hwnd, PREVIEW_TIMER);
            }
            LRESULT(0)
        }
        WM_SETCURSOR if (l.0 & 0xffff) as u32 == HTCLIENT => {
            let clickable = dlg
                .mouse
                .and_then(|(x, y)| dlg.hit_at(x, y))
                .is_some_and(|h| h != Hit::Dismiss);
            let cursor = LoadCursorW(None, if clickable { IDC_HAND } else { IDC_ARROW });
            if let Ok(cursor) = cursor {
                SetCursor(cursor);
            }
            LRESULT(1)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN if dlg.capture_pinch => {
            dlg.capture_key(w.0 as u32);
            dlg.render();
            LRESULT(0)
        }
        WM_KEYDOWN if w.0 == 0x1b => {
            if dlg.open_combo.take().is_none() {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            dlg.render();
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse_xy(l);
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
            if let Some(slider) = dlg.dragging {
                dlg.set_slider_from_x(slider, x);
            }
            dlg.render();
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            dlg.mouse = None;
            dlg.render();
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_xy(l);
            match dlg.hit_at(x, y) {
                Some(Hit::Slider(slider)) => {
                    dlg.dragging = Some(slider);
                    SetCapture(hwnd);
                    dlg.set_slider_from_x(slider, x);
                }
                Some(hit) => dlg.activate(hit),
                None if y < 56.0 => {
                    // Empty title area: drag the window.
                    let _ = ReleaseCapture();
                    let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                }
                None => {}
            }
            dlg.render();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if dlg.dragging.take().is_some() {
                let _ = ReleaseCapture();
                dlg.persist("Saved.");
            }
            dlg.render();
            LRESULT(0)
        }
        WM_SIZE => {
            let width = (l.0 & 0xffff) as u32;
            let height = ((l.0 >> 16) & 0xffff) as u32;
            if width > 0 && height > 0 {
                dlg.width = width;
                dlg.height = height;
                dlg.d2d.resize(width, height);
                dlg.render();
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            dlg.stop_capture();
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

impl SettingsDialog {
    pub fn new(
        settings: Arc<parking_lot::RwLock<Settings>>,
        event_tx: Sender<AppEvent>,
        main_hwnd: usize,
    ) -> Result<Box<Self>, windows::core::Error> {
        unsafe {
            let instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherSettingsWndClass");
            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(settings_wnd_proc);
            wc.hInstance = instance.into();
            wc.lpszClassName = class_name;
            wc.hIcon = LoadIconW(instance, PCWSTR(1 as *const u16)).unwrap_or_default();
            wc.hIconSm = wc.hIcon;
            wc.hCursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
            let _ = RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_APPWINDOW,
                class_name,
                w!("AppSwitcher — Settings"),
                WS_POPUP,
                140,
                110,
                WINDOW_W,
                WINDOW_H,
                HWND(std::ptr::null_mut()),
                None,
                instance,
                None,
            )?;
            // Dark title/frame and Windows 11 rounded corners with a shadow.
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
            let width = (client.right - client.left).max(900) as u32;
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
                status: "Changes save automatically.".into(),
                open_titles: Vec::new(),
                window_page: 0,
                capture_pinch: false,
                autostart: autostart_enabled(),
                touchpad_warning: three_finger_enabled(),
                page: Page::Appearance,
                open_combo: None,
                dragging: None,
                mouse: None,
                epoch: std::time::Instant::now(),
                hits: RefCell::new(Vec::new()),
                tracks: RefCell::new(Vec::new()),
                combo_anchor: Cell::new(None),
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, dlg.as_mut() as *mut _ as isize);
            Ok(dlg)
        }
    }

    pub fn show(&mut self) {
        // ~60fps redraw for the live previews; stopped in WM_TIMER once hidden.
        unsafe {
            SetTimer(self.hwnd, PREVIEW_TIMER, 16, None);
        }
        self.refresh_windows();
        self.touchpad_warning = three_finger_enabled();
        self.autostart = autostart_enabled();
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetWindowPos(self.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = SetForegroundWindow(self.hwnd);
        }
        self.render();
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
        let pages = self.open_titles.len().div_ceil(WINDOWS_PER_PAGE).max(1);
        self.window_page = self.window_page.min(pages - 1);
    }

    fn persist(&mut self, msg: &str) {
        // Push live-tunable values to the rest of the app immediately.
        crate::input::set_sensitivity(self.settings.read().sensitivity);
        crate::animations::set_respect_system_animations(
            self.settings.read().respect_system_animations,
        );
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

    fn stop_capture(&mut self) {
        if self.capture_pinch {
            self.capture_pinch = false;
            crate::input::keyboard_hook::set_enabled(self.settings.read().alt_tab);
        }
    }

    /// A key pressed while recording the pinch shortcut.
    fn capture_key(&mut self, vk: u32) {
        if vk == 0x1b {
            self.stop_capture();
            self.status = "Recording cancelled.".into();
            return;
        }
        if matches!(vk, 0x10 | 0x11 | 0x12 | 0x5b | 0x5c | 0xa0..=0xa5) {
            return; // wait for the non-modifier key
        }
        {
            let mut s = self.settings.write();
            s.pinch_mods.clear();
            for k in [0x11, 0x12, 0x10, 0x5b] {
                if unsafe { GetAsyncKeyState(k) as u16 & 0x8000 } != 0 {
                    s.pinch_mods.push(k as u32);
                }
            }
            s.pinch_vk = vk;
        }
        self.stop_capture();
        self.persist("Pinch shortcut updated.");
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| inside(*r, x, y))
            .map(|(_, h)| *h)
    }

    fn set_slider_from_x(&mut self, slider: Slider, x: f32) {
        let Some(track) = self
            .tracks
            .borrow()
            .iter()
            .find(|(s, _)| *s == slider)
            .map(|(_, r)| *r)
        else {
            return;
        };
        let t = ((x - track.left) / (track.right - track.left)).clamp(0.0, 1.0) as f64;
        let mut s = self.settings.write();
        match slider {
            Slider::Duration => s.duration = ((80.0 + 520.0 * t) / 10.0).round() * 10.0 / 1000.0,
            Slider::Magnification => s.dock_mag = ((1.2 + 1.4 * t) * 20.0).round() / 20.0,
            Slider::Sensitivity => s.sensitivity = ((120.0 + 280.0 * t) / 5.0).round() as i32 * 5,
        }
        if slider == Slider::Sensitivity {
            crate::input::set_sensitivity(s.sensitivity);
        }
    }

    fn activate(&mut self, hit: Hit) {
        match hit {
            Hit::Nav(page) => {
                self.page = page;
                self.open_combo = None;
                if page == Page::Hidden {
                    self.refresh_windows();
                }
            }
            Hit::Close => {
                self.stop_capture();
                self.open_combo = None;
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
            }
            Hit::Combo(c) => {
                self.open_combo = if self.open_combo == Some(c) { None } else { Some(c) };
            }
            Hit::Dismiss => self.open_combo = None,
            Hit::ComboItem(c, i) => {
                {
                    let mut s = self.settings.write();
                    match c {
                        Combo::Animation => s.animation = ANIMATIONS[i].0.into(),
                        Combo::Layout => s.layout = LAYOUTS[i].0.into(),
                        Combo::HotkeyMod => s.hotkey_mod = if i == 0 { "alt" } else { "ctrl" }.into(),
                        Combo::HotkeyKey => s.hotkey_key = HOTKEYS[i].1,
                    }
                    if matches!(c, Combo::HotkeyMod | Combo::HotkeyKey) {
                        crate::input::keyboard_hook::configure(&s.hotkey_mod, s.hotkey_key);
                    }
                }
                self.open_combo = None;
                self.persist("Saved.");
            }
            Hit::Slider(_) => {}
            Hit::Toggle(t) => match t {
                Toggle::AltTab => {
                    let on = {
                        let mut s = self.settings.write();
                        s.alt_tab = !s.alt_tab;
                        s.alt_tab
                    };
                    crate::input::keyboard_hook::set_enabled(on);
                    self.persist(if on {
                        "Alt+Tab now opens AppSwitcher."
                    } else {
                        "Alt+Tab goes to Windows again."
                    });
                }
                Toggle::Autostart => {
                    let on = !self.autostart;
                    if set_autostart(on) {
                        self.autostart = on;
                        self.status = if on {
                            "Starts with Windows.".into()
                        } else {
                            "Won't start with Windows.".into()
                        };
                    } else {
                        self.status = "Couldn't change the startup setting.".into();
                    }
                }
                Toggle::Pinch => {
                    {
                        let mut s = self.settings.write();
                        s.pinch_enabled = !s.pinch_enabled;
                    }
                    self.persist("Saved.");
                }
                Toggle::FollowWindowsAnimations => {
                    {
                        let mut s = self.settings.write();
                        s.respect_system_animations = !s.respect_system_animations;
                    }
                    self.persist("Saved.");
                }
            },
            Hit::Accent(i) => {
                self.settings.write().accent = ACCENTS[i].1;
                self.persist("Accent updated.");
            }
            Hit::PinchRecord => {
                self.capture_pinch = true;
                crate::input::keyboard_hook::set_enabled(false);
                self.status = "Press the shortcut keys. Esc cancels.".into();
                unsafe {
                    let _ = SetFocus(self.hwnd);
                }
            }
            Hit::PinchClear => {
                {
                    let mut s = self.settings.write();
                    s.pinch_mods.clear();
                    s.pinch_vk = 0;
                }
                self.persist("Pinch shortcut cleared.");
            }
            Hit::PreviewPicker => {
                let _ = self.event_tx.send(AppEvent::PreviewPicker);
                self.notify_main();
            }
            Hit::PreviewAnimation => {
                let _ = self.event_tx.send(AppEvent::PreviewAnimation);
                self.notify_main();
            }
            Hit::HideWindow(i) => {
                if let Some(t) = self.open_titles.get(i).cloned() {
                    {
                        let mut s = self.settings.write();
                        if s.blocklist.contains(&t) {
                            s.blocklist.retain(|v| v != &t);
                        } else {
                            s.blocklist.push(t);
                        }
                    }
                    self.persist("Saved.");
                }
            }
            Hit::PagePrev => self.window_page = self.window_page.saturating_sub(1),
            Hit::PageNext => {
                let pages = self.open_titles.len().div_ceil(WINDOWS_PER_PAGE).max(1);
                self.window_page = (self.window_page + 1).min(pages - 1);
            }
            Hit::Refresh => {
                self.refresh_windows();
                self.status = "Window list refreshed.".into();
            }
            Hit::WarnOpen => unsafe {
                let _ = ShellExecuteW(
                    self.hwnd,
                    w!("open"),
                    w!("ms-settings:devices-touchpad"),
                    None,
                    None,
                    SW_SHOWNORMAL,
                );
            },
            Hit::WarnDismiss => {
                self.settings.write().warn_dismissed = true;
                self.persist("Notice dismissed.");
            }
            Hit::OpenConfigFolder => {
                if let Some(dir) = Settings::config_path().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
                    let _ = std::fs::create_dir_all(&dir);
                    let wide: Vec<u16> = format!("{}\0", dir.display()).encode_utf16().collect();
                    unsafe {
                        let _ = ShellExecuteW(
                            self.hwnd,
                            w!("open"),
                            PCWSTR(wide.as_ptr()),
                            None,
                            None,
                            SW_SHOWNORMAL,
                        );
                    }
                }
            }
        }
    }

    // ── Drawing primitives ───────────────────────────────────────────────

    fn hit(&self, r: D2D_RECT_F, hit: Hit) {
        self.hits.borrow_mut().push((r, hit));
    }

    fn hovered(&self, r: D2D_RECT_F) -> bool {
        self.dragging.is_none() && self.mouse.is_some_and(|(x, y)| inside(r, x, y))
    }

    fn accent(&self) -> D2D1_COLOR_F {
        let c = self.settings.read().accent;
        rgba(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0)
    }

    fn text(&self, s: &str, r: D2D_RECT_F, color: D2D1_COLOR_F, size: f32, weight: u32, align: Align) {
        self.d2d.text(s, r, color, size, weight, Font::Text, align);
    }

    fn content_left(&self) -> f32 {
        SIDEBAR_W + CONTENT_PAD
    }

    fn content_right(&self) -> f32 {
        self.width as f32 - CONTENT_PAD
    }

    /// A setting row: icon tile, title and description on the left. Returns
    /// the row rect; the caller draws the control on the right.
    fn row(&self, top: f32, glyph_char: char, title: &str, desc: &str) -> D2D_RECT_F {
        let r = rect(self.content_left(), top, self.content_right(), top + ROW_H);
        let fill = if self.hovered(r) { theme::CARD_HOVER } else { theme::CARD };
        self.d2d.fill_rounded_rect(r, 10.0, fill);
        self.d2d.draw_rounded_rect(r, 10.0, theme::STROKE, 1.0);
        let cy = (r.top + r.bottom) * 0.5;
        let tile = rect(r.left + 16.0, cy - 18.0, r.left + 52.0, cy + 18.0);
        self.d2d.fill_rounded_rect(tile, 9.0, rgba(1.0, 1.0, 1.0, 0.05));
        self.d2d.icon(glyph_char, tile, theme::TEXT2, 15.0);
        let text_right = r.right - 320.0;
        self.text(title, rect(r.left + 68.0, cy - 21.0, text_right, cy + 1.0), theme::TEXT1, 14.0, 500, Align::Leading);
        self.text(desc, rect(r.left + 68.0, cy + 1.0, text_right, cy + 21.0), theme::TEXT3, 12.5, 400, Align::Leading);
        r
    }

    fn row_top(&self, index: usize, first: f32) -> f32 {
        first + index as f32 * (ROW_H + ROW_GAP)
    }

    fn combo(&self, r: D2D_RECT_F, value: &str, id: Combo) {
        let ac = self.accent();
        let open = self.open_combo == Some(id);
        let fill = if open || self.hovered(r) { theme::CONTROL_HOVER } else { theme::CONTROL };
        self.d2d.fill_rounded_rect(r, 8.0, fill);
        self.d2d
            .draw_rounded_rect(r, 8.0, if open { ac } else { theme::CONTROL_STROKE }, 1.0);
        self.text(value, rect(r.left + 14.0, r.top, r.right - 36.0, r.bottom), theme::TEXT1, 13.5, 500, Align::Leading);
        self.d2d.icon(
            if open { glyph::CHEVRON_UP } else { glyph::CHEVRON_DOWN },
            rect(r.right - 34.0, r.top, r.right - 10.0, r.bottom),
            theme::TEXT2,
            10.0,
        );
        self.hit(r, Hit::Combo(id));
        if open {
            self.combo_anchor.set(Some(r));
        }
    }

    /// Windows 11 toggle with an On/Off label, right-aligned at `right`.
    fn toggle(&self, right: f32, cy: f32, on: bool, id: Toggle) {
        let ac = self.accent();
        let track = rect(right - 44.0, cy - 11.0, right, cy + 11.0);
        let hit = rect(right - 90.0, cy - 16.0, right + 4.0, cy + 16.0);
        let hover = self.hovered(hit);
        self.text(
            if on { "On" } else { "Off" },
            rect(right - 90.0, cy - 10.0, right - 54.0, cy + 10.0),
            theme::TEXT2,
            13.0,
            400,
            Align::Trailing,
        );
        if on {
            self.d2d.fill_rounded_gradient(
                track,
                11.0,
                (track.left, track.top),
                (track.left, track.bottom),
                &[(0.0, lighten(ac, 0.10)), (1.0, ac)],
            );
            self.d2d
                .fill_circle(track.right - 11.0, cy, if hover { 8.0 } else { 7.0 }, on_accent(ac));
        } else {
            self.d2d.fill_rounded_rect(
                track,
                11.0,
                if hover { rgba(1.0, 1.0, 1.0, 0.07) } else { rgba(0.0, 0.0, 0.0, 0.2) },
            );
            self.d2d.draw_rounded_rect(track, 11.0, rgba(1.0, 1.0, 1.0, 0.45), 1.0);
            self.d2d
                .fill_circle(track.left + 11.0, cy, if hover { 7.0 } else { 6.0 }, rgba(1.0, 1.0, 1.0, 0.75));
        }
        self.hit(hit, Hit::Toggle(id));
    }

    /// Draggable slider ending at `right`, with its value shown to the left.
    fn slider(&self, right: f32, cy: f32, t: f32, value: &str, id: Slider) {
        let ac = self.accent();
        let track = rect(right - 230.0, cy - 2.5, right - 12.0, cy + 2.5);
        self.text(value, rect(right - 320.0, cy - 10.0, track.left - 18.0, cy + 10.0), theme::TEXT1, 13.0, 600, Align::Trailing);
        let cx = track.left + (track.right - track.left) * t.clamp(0.0, 1.0);
        self.d2d.fill_rounded_rect(track, 2.5, rgba(1.0, 1.0, 1.0, 0.12));
        if cx > track.left + 1.0 {
            self.d2d.fill_rounded_gradient(
                rect(track.left, track.top, cx, track.bottom),
                2.5,
                (track.left, cy),
                (cx, cy),
                &[(0.0, with_alpha(ac, 0.5)), (1.0, ac)],
            );
        }
        let hit = rect(track.left - 12.0, cy - 16.0, track.right + 12.0, cy + 16.0);
        let active = self.dragging == Some(id) || self.hovered(hit);
        self.d2d.fill_circle(cx, cy + 1.0, 11.5, rgba(0.0, 0.0, 0.0, 0.35));
        self.d2d.fill_circle(cx, cy, 11.0, rgba(0.21, 0.21, 0.24, 1.0));
        self.d2d.draw_rounded_rect(rect(cx - 11.0, cy - 11.0, cx + 11.0, cy + 11.0), 11.0, rgba(1.0, 1.0, 1.0, 0.12), 1.0);
        self.d2d.fill_circle(cx, cy, if active { 7.5 } else { 6.0 }, ac);
        self.tracks.borrow_mut().push((id, track));
        self.hit(hit, Hit::Slider(id));
    }

    fn button(&self, r: D2D_RECT_F, label: &str, glyph_char: Option<char>, hit: Hit, primary: bool) {
        let ac = self.accent();
        let hover = self.hovered(r);
        if primary {
            let base = if hover { lighten(ac, 0.06) } else { ac };
            self.d2d.fill_rounded_gradient(
                r,
                8.0,
                (r.left, r.top),
                (r.left, r.bottom),
                &[(0.0, lighten(base, 0.08)), (1.0, base)],
            );
        } else {
            self.d2d
                .fill_rounded_rect(r, 8.0, if hover { theme::CONTROL_HOVER } else { theme::CONTROL });
            self.d2d.draw_rounded_rect(r, 8.0, theme::CONTROL_STROKE, 1.0);
        }
        let fg = if primary { on_accent(ac) } else { theme::TEXT1 };
        let size = 13.0;
        let text_w = self.d2d.measure(label, size, 500, Font::Text);
        let icon_w = if glyph_char.is_some() { 22.0 } else { 0.0 };
        let left = (r.left + r.right - text_w - icon_w) * 0.5;
        if let Some(g) = glyph_char {
            self.d2d.icon(g, rect(left, r.top, left + 14.0, r.bottom), fg, 12.0);
        }
        self.text(label, rect(left + icon_w, r.top, r.right, r.bottom), fg, size, 500, Align::Leading);
        self.hit(r, hit);
    }

    fn keycaps(&self, chord: &str, right: f32, cy: f32) -> f32 {
        let keys: Vec<&str> = chord.split(" + ").filter(|k| !k.is_empty()).collect();
        if keys.is_empty() {
            self.text("Not set", rect(right - 160.0, cy - 10.0, right, cy + 10.0), theme::TEXT3, 13.0, 400, Align::Trailing);
            return right - 60.0;
        }
        let size = 12.5;
        let widths: Vec<f32> = keys
            .iter()
            .map(|k| self.d2d.measure(k, size, 600, Font::Text) + 20.0)
            .collect();
        let plus = 18.0;
        let total: f32 = widths.iter().sum::<f32>() + plus * (keys.len() - 1) as f32;
        let mut x = right - total;
        let start = x;
        for (i, (key, w)) in keys.iter().zip(&widths).enumerate() {
            if i > 0 {
                self.text("+", rect(x, cy - 10.0, x + plus, cy + 10.0), theme::TEXT3, 12.0, 400, Align::Center);
                x += plus;
            }
            let cap = rect(x, cy - 14.0, x + w, cy + 14.0);
            self.d2d.fill_rounded_rect(cap, 6.0, rgba(1.0, 1.0, 1.0, 0.08));
            self.d2d.draw_rounded_rect(cap, 6.0, rgba(1.0, 1.0, 1.0, 0.12), 1.0);
            self.d2d.fill_rect(rect(cap.left + 5.0, cap.bottom - 1.5, cap.right - 5.0, cap.bottom), rgba(0.0, 0.0, 0.0, 0.45));
            self.text(key, cap, theme::TEXT1, size, 600, Align::Center);
            x += w;
        }
        start
    }

    /// Small app-window mockup used by the live previews.
    fn mini_window(&self, r: D2D_RECT_F, tint: D2D1_COLOR_F, alpha: f32) {
        let w = r.right - r.left;
        let h = r.bottom - r.top;
        if w < 2.0 || h < 2.0 || alpha <= 0.01 {
            return;
        }
        let radius = (h * 0.07).clamp(2.0, 8.0);
        self.d2d.fill_rounded_rect(r, radius, with_alpha(tint, alpha));
        let bar = rect(r.left, r.top, r.right, r.top + (h * 0.15).max(3.0));
        self.d2d.fill_rounded_rect(bar, radius, rgba(0.0, 0.0, 0.0, 0.25 * alpha));
        let dot = (h * 0.035).clamp(1.0, 3.5);
        for i in 0..3 {
            self.d2d.fill_circle(
                r.left + w * 0.05 + i as f32 * dot * 3.0,
                (bar.top + bar.bottom) * 0.5,
                dot,
                rgba(1.0, 1.0, 1.0, 0.35 * alpha),
            );
        }
        for (i, frac) in [0.72f32, 0.55, 0.64].iter().enumerate() {
            let top = r.top + h * (0.30 + i as f32 * 0.17);
            let line_h = (h * 0.065).max(1.5);
            if top + line_h > r.bottom - 2.0 {
                break;
            }
            self.d2d.fill_rounded_rect(
                rect(r.left + w * 0.08, top, r.left + w * (0.08 + 0.8 * frac), top + line_h),
                line_h * 0.5,
                rgba(1.0, 1.0, 1.0, 0.26 * alpha),
            );
        }
    }

    /// Looping demo of a switch animation inside `stage`.
    fn transition_stage(&self, stage: D2D_RECT_F, kind: &str, duration: f32) {
        let ac = self.accent();
        self.d2d.fill_rounded_rect(stage, 8.0, rgba(0.03, 0.03, 0.04, 1.0));
        let duration = duration.max(0.08);
        let cycle = duration + 1.1;
        let elapsed = self.epoch.elapsed().as_secs_f32() % (cycle * 2.0);
        let (forward, local) = if elapsed < cycle { (true, elapsed) } else { (false, elapsed - cycle) };
        let t = ((local - 0.55) / duration).clamp(0.0, 1.0);
        let e = 1.0 - (1.0 - t).powi(3);

        let tint_a = rgba(ac.r * 0.6, ac.g * 0.6, ac.b * 0.6, 1.0);
        let tint_b = rgba(0.27, 0.29, 0.35, 1.0);
        let (from, to) = if forward { (tint_a, tint_b) } else { (tint_b, tint_a) };
        let full = inset(stage, 10.0, 10.0);
        let x0 = full.left;
        let w = full.right - full.left;
        let shift = |dx: f32| rect(full.left + dx, full.top, full.right + dx, full.bottom);
        let scaled = |s: f32| {
            let cx = (full.left + full.right) * 0.5;
            let cy = (full.top + full.bottom) * 0.5;
            let hw = (full.right - full.left) * 0.5 * s;
            let hh = (full.bottom - full.top) * 0.5 * s;
            rect(cx - hw, cy - hh, cx + hw, cy + hh)
        };

        self.d2d.push_clip(full);
        match kind {
            "push" => {
                self.mini_window(shift(-0.3 * w * e), from, 1.0 - 0.5 * e);
                self.mini_window(shift(w * (1.0 - e)), to, 1.0);
            }
            "fade" => {
                self.mini_window(full, from, 1.0);
                self.mini_window(full, to, e);
            }
            "zoom" => {
                self.mini_window(scaled(1.0 + 0.3 * e), from, 1.0 - e);
                self.mini_window(scaled(0.7 + 0.3 * e), to, e);
            }
            "stack" => {
                self.mini_window(scaled(1.0 - 0.12 * e), from, 1.0 - 0.4 * e);
                self.mini_window(shift(w * (1.0 - e)), to, 1.0);
            }
            "cube" => {
                let edge = x0 + w * (1.0 - e);
                self.mini_window(rect(full.left, full.top, edge, full.bottom), from, 1.0 - 0.4 * e);
                self.mini_window(rect(edge, full.top, full.right, full.bottom), to, 0.6 + 0.4 * e);
            }
            "flip" => {
                let cx = x0 + w * 0.5;
                let (tint, half) = if e < 0.5 {
                    (from, w * 0.5 * (1.0 - 2.0 * e))
                } else {
                    (to, w * 0.5 * (2.0 * e - 1.0))
                };
                self.mini_window(rect(cx - half, full.top, cx + half, full.bottom), tint, 1.0);
            }
            "duo_fold" => {
                self.mini_window(scaled(0.85 + 0.15 * e), to, e);
                let half = w * 0.5 * (1.0 - e);
                self.mini_window(rect(full.left, full.top, x0 + half, full.bottom), from, 1.0 - 0.3 * e);
                self.mini_window(rect(full.right - half, full.top, full.right, full.bottom), from, 1.0 - 0.3 * e);
            }
            _ => {
                self.mini_window(shift(-w * e), from, 1.0);
                self.mini_window(shift(w * (1.0 - e)), to, 1.0);
            }
        }
        self.d2d.pop_clip();
        self.d2d.draw_rounded_rect(stage, 8.0, theme::STROKE, 1.0);
    }

    /// Live dock with magnification `mag`, focus gliding across it.
    fn dock_stage(&self, stage: D2D_RECT_F, mag: f32) {
        let ac = self.accent();
        self.d2d.fill_rounded_rect(stage, 8.0, rgba(0.03, 0.03, 0.04, 1.0));
        self.d2d.draw_rounded_rect(stage, 8.0, theme::STROKE, 1.0);
        let area = inset(stage, 16.0, 14.0);
        let n = 5;
        let (base_w, base_h, gap) = (34.0f32, 22.0f32, 7.0f32);
        let focus = 2.0 + 1.9 * (self.epoch.elapsed().as_secs_f32() * 1.1).sin();
        let mags: Vec<f32> = (0..n)
            .map(|i| {
                let d = i as f32 - focus;
                1.0 + (mag - 1.0) / (1.0 + d * d * 1.5)
            })
            .collect();
        let total: f32 = mags.iter().map(|m| base_w * m).sum::<f32>() + gap * (n - 1) as f32;
        let max_h = base_h * mag.max(1.0);
        let scale = ((area.right - area.left) / total)
            .min((area.bottom - area.top - 12.0) / max_h)
            .max(0.2);
        let baseline = (area.top + area.bottom) * 0.5 + max_h * scale * 0.5;
        let mut x = (area.left + area.right - total * scale) * 0.5;
        self.d2d.fill_rounded_rect(
            rect(x - 10.0, baseline - base_h * scale - 10.0, x + total * scale + 10.0, baseline + 8.0),
            10.0,
            rgba(1.0, 1.0, 1.0, 0.05),
        );
        let nearest = focus.round().clamp(0.0, (n - 1) as f32) as usize;
        for (i, m) in mags.iter().enumerate() {
            let cw = base_w * m * scale;
            let ch = base_h * m * scale;
            let r = rect(x, baseline - ch, x + cw, baseline);
            if i == nearest {
                self.mini_window(r, rgba(ac.r * 0.6, ac.g * 0.6, ac.b * 0.6, 1.0), 1.0);
                self.d2d.draw_rounded_rect(inset(r, -3.0, -3.0), 8.0, ac, 1.5);
            } else {
                self.mini_window(r, rgba(0.27, 0.29, 0.35, 1.0), 0.9);
            }
            x += cw + gap * scale;
        }
    }

    // ── Frame ────────────────────────────────────────────────────────────

    pub fn render(&self) {
        self.hits.borrow_mut().clear();
        self.tracks.borrow_mut().clear();
        self.combo_anchor.set(None);
        let w = self.width as f32;
        let h = self.height as f32;
        let ac = self.accent();

        self.d2d.begin_draw();
        self.d2d.clear(theme::BACKGROUND.r, theme::BACKGROUND.g, theme::BACKGROUND.b, 1.0);
        // Soft accent glow behind the page header.
        self.d2d.fill_rounded_gradient(
            rect(SIDEBAR_W, 0.0, w, h),
            0.0,
            (w, 0.0),
            (w - 520.0, 340.0),
            &[(0.0, with_alpha(ac, 0.16)), (0.5, with_alpha(ac, 0.04)), (1.0, with_alpha(ac, 0.0))],
        );

        self.draw_sidebar(h);
        self.draw_header(w);
        match self.page {
            Page::Appearance => self.page_appearance(),
            Page::Motion => self.page_motion(),
            Page::Gestures => self.page_gestures(),
            Page::Keyboard => self.page_keyboard(),
            Page::Hidden => self.page_hidden(),
            Page::General => self.page_general(),
        }
        self.draw_flyout(w, h);
        let _ = self.d2d.end_draw();
    }

    fn draw_sidebar(&self, h: f32) {
        let ac = self.accent();
        self.d2d.fill_rect(rect(0.0, 0.0, SIDEBAR_W, h), theme::SIDEBAR);
        self.d2d.fill_rect(rect(SIDEBAR_W - 1.0, 0.0, SIDEBAR_W, h), theme::STROKE);

        // Brand
        let tile = rect(22.0, 24.0, 58.0, 60.0);
        self.d2d.fill_rounded_gradient(
            tile,
            10.0,
            (tile.left, tile.top),
            (tile.right, tile.bottom),
            &[(0.0, lighten(ac, 0.08)), (1.0, rgba(ac.r * 0.55, ac.g * 0.55, ac.b * 0.55, 1.0))],
        );
        self.d2d.fill_rounded_rect(rect(32.0, 37.0, 50.0, 51.0), 3.0, rgba(0.0, 0.0, 0.0, 0.30));
        self.d2d.fill_rounded_rect(rect(29.0, 32.0, 47.0, 46.0), 3.0, rgba(1.0, 1.0, 1.0, 0.92));
        self.d2d.text("AppSwitcher", rect(70.0, 24.0, SIDEBAR_W - 12.0, 44.0), theme::TEXT1, 16.5, 700, Font::Display, Align::Leading);
        self.text(
            &format!("Version {}", env!("CARGO_PKG_VERSION")),
            rect(70.0, 43.0, SIDEBAR_W - 12.0, 60.0),
            theme::TEXT3,
            11.5,
            400,
            Align::Leading,
        );

        // Navigation
        let warn = self.touchpad_warning && !self.settings.read().warn_dismissed;
        for (i, (page, g, label, _)) in PAGES.iter().enumerate() {
            let r = rect(12.0, 92.0 + i as f32 * 42.0, SIDEBAR_W - 12.0, 92.0 + i as f32 * 42.0 + 38.0);
            let active = self.page == *page;
            if active {
                self.d2d.fill_rounded_rect(r, 8.0, rgba(1.0, 1.0, 1.0, 0.07));
                let cy = (r.top + r.bottom) * 0.5;
                self.d2d.fill_rounded_rect(rect(r.left, cy - 9.0, r.left + 3.0, cy + 9.0), 1.5, ac);
            } else if self.hovered(r) {
                self.d2d.fill_rounded_rect(r, 8.0, rgba(1.0, 1.0, 1.0, 0.04));
            }
            self.d2d.icon(*g, rect(r.left + 12.0, r.top, r.left + 36.0, r.bottom), if active { ac } else { theme::TEXT2 }, 15.0);
            self.text(
                label,
                rect(r.left + 46.0, r.top, r.right - 10.0, r.bottom),
                if active { theme::TEXT1 } else { theme::TEXT2 },
                13.5,
                if active { 600 } else { 400 },
                Align::Leading,
            );
            if warn && *page == Page::Gestures {
                self.d2d.fill_circle(r.right - 16.0, (r.top + r.bottom) * 0.5, 4.0, theme::AMBER);
            }
            self.hit(r, Hit::Nav(*page));
        }

        // Status card
        let card = rect(12.0, h - 82.0, SIDEBAR_W - 12.0, h - 16.0);
        self.d2d.fill_rounded_rect(card, 10.0, rgba(1.0, 1.0, 1.0, 0.035));
        self.d2d.draw_rounded_rect(card, 10.0, theme::STROKE, 1.0);
        let paused = crate::input::is_paused();
        let (dot, state) = if paused {
            (theme::AMBER, "Paused")
        } else {
            (theme::GREEN, "Running")
        };
        self.d2d.fill_circle(card.left + 20.0, card.top + 22.0, 7.0, with_alpha(dot, 0.25));
        self.d2d.fill_circle(card.left + 20.0, card.top + 22.0, 4.0, dot);
        self.text(state, rect(card.left + 34.0, card.top + 12.0, card.right - 12.0, card.top + 32.0), theme::TEXT1, 13.0, 600, Align::Leading);
        self.text(&self.status, rect(card.left + 14.0, card.top + 36.0, card.right - 12.0, card.bottom - 10.0), theme::TEXT3, 12.0, 400, Align::Leading);
    }

    fn draw_header(&self, w: f32) {
        let (_, _, title, subtitle) = PAGES.iter().find(|(p, ..)| *p == self.page).copied().unwrap_or(PAGES[0]);
        let left = self.content_left();
        self.d2d.text(title, rect(left, 30.0, w - 120.0, 68.0), theme::TEXT1, 28.0, 700, Font::Display, Align::Leading);
        self.text(subtitle, rect(left, 68.0, w - 120.0, 90.0), theme::TEXT2, 13.5, 400, Align::Leading);
        let close = rect(w - 54.0, 14.0, w - 14.0, 54.0);
        if self.hovered(close) {
            self.d2d.fill_rounded_rect(close, 8.0, rgba(0.90, 0.20, 0.22, 0.85));
        }
        self.d2d.icon(glyph::CLOSE, close, theme::TEXT1, 11.0);
        self.hit(close, Hit::Close);
    }

    const BODY_TOP: f32 = 116.0;

    fn page_appearance(&self) {
        let c = self.settings.read().clone();
        let ac = self.accent();
        let anim_name = ANIMATIONS.iter().find(|(k, _)| *k == c.animation).map(|(_, v)| *v).unwrap_or("Slide");
        let layout_name = LAYOUTS.iter().find(|(k, _)| *k == c.layout).map(|(_, v)| *v).unwrap_or("Dock");

        // Hero card: live preview of the selected transition.
        let hero = rect(self.content_left(), Self::BODY_TOP, self.content_right(), Self::BODY_TOP + 196.0);
        self.d2d.fill_rounded_gradient(
            hero,
            12.0,
            (hero.left, hero.top),
            (hero.right, hero.bottom),
            &[(0.0, rgba(ac.r * 0.22, ac.g * 0.22, ac.b * 0.22, 1.0)), (0.6, theme::CARD), (1.0, theme::CARD)],
        );
        self.d2d.draw_rounded_rect(hero, 12.0, theme::STROKE, 1.0);
        self.text("LIVE PREVIEW", rect(hero.left + 28.0, hero.top + 26.0, hero.left + 300.0, hero.top + 44.0), ac, 11.0, 700, Align::Leading);
        self.d2d.text(anim_name, rect(hero.left + 28.0, hero.top + 46.0, hero.left + 340.0, hero.top + 84.0), theme::TEXT1, 26.0, 700, Font::Display, Align::Leading);
        self.text(
            &format!("{} ms · {} picker", (c.duration * 1000.0).round() as i32, layout_name),
            rect(hero.left + 28.0, hero.top + 86.0, hero.left + 340.0, hero.top + 106.0),
            theme::TEXT2,
            13.0,
            400,
            Align::Leading,
        );
        self.button(rect(hero.left + 28.0, hero.bottom - 60.0, hero.left + 178.0, hero.bottom - 24.0), "Play on screen", Some(glyph::PLAY), Hit::PreviewAnimation, true);
        self.button(rect(hero.left + 188.0, hero.bottom - 60.0, hero.left + 338.0, hero.bottom - 24.0), "Open picker", Some(glyph::VIEW_ALL), Hit::PreviewPicker, false);
        let stage_h = hero.bottom - hero.top - 40.0;
        let stage = rect(hero.right - 20.0 - stage_h * 1.65, hero.top + 20.0, hero.right - 20.0, hero.bottom - 20.0);
        self.transition_stage(stage, &c.animation, c.duration as f32);

        let first = hero.bottom + 20.0;
        let r = self.row(self.row_top(0, first), glyph::PLAY, "Switch animation", "Plays when you swipe or Alt+Tab to another app");
        let cy = (r.top + r.bottom) * 0.5;
        self.combo(rect(r.right - 220.0, cy - 17.0, r.right - 20.0, cy + 17.0), anim_name, Combo::Animation);

        let r = self.row(self.row_top(1, first), glyph::VIEW_ALL, "Picker layout", "How the app picker arranges your windows");
        let cy = (r.top + r.bottom) * 0.5;
        self.combo(rect(r.right - 220.0, cy - 17.0, r.right - 20.0, cy + 17.0), layout_name, Combo::Layout);

        let r = self.row(self.row_top(2, first), glyph::COLOR, "Accent color", "Used for highlights, selection and toggles");
        let cy = (r.top + r.bottom) * 0.5;
        for (i, (_, rgb)) in ACCENTS.iter().enumerate().rev() {
            let cx = r.right - 34.0 - (ACCENTS.len() - 1 - i) as f32 * 32.0;
            let col = rgba(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, 1.0);
            let hit = rect(cx - 14.0, cy - 14.0, cx + 14.0, cy + 14.0);
            let selected = c.accent == *rgb;
            if selected {
                self.d2d.draw_rounded_rect(rect(cx - 15.0, cy - 15.0, cx + 15.0, cy + 15.0), 15.0, theme::TEXT1, 1.5);
            }
            self.d2d.fill_circle(cx, cy, if self.hovered(hit) && !selected { 12.0 } else { 11.0 }, col);
            if selected {
                self.d2d.icon(glyph::CHECK, hit, on_accent(col), 11.0);
            }
            self.hit(hit, Hit::Accent(i));
        }
    }

    fn page_motion(&self) {
        let c = self.settings.read().clone();
        let ac = self.accent();
        let hero = rect(self.content_left(), Self::BODY_TOP, self.content_right(), Self::BODY_TOP + 176.0);
        self.d2d.fill_rounded_gradient(
            hero,
            12.0,
            (hero.left, hero.top),
            (hero.right, hero.bottom),
            &[(0.0, rgba(ac.r * 0.22, ac.g * 0.22, ac.b * 0.22, 1.0)), (0.6, theme::CARD), (1.0, theme::CARD)],
        );
        self.d2d.draw_rounded_rect(hero, 12.0, theme::STROKE, 1.0);
        self.text("LIVE PREVIEW", rect(hero.left + 28.0, hero.top + 26.0, hero.left + 300.0, hero.top + 44.0), ac, 11.0, 700, Align::Leading);
        self.d2d.text(&format!("{:.2}×", c.dock_mag), rect(hero.left + 28.0, hero.top + 46.0, hero.left + 300.0, hero.top + 88.0), theme::TEXT1, 32.0, 700, Font::Display, Align::Leading);
        self.text("Dock magnification. Drag the slider below.", rect(hero.left + 28.0, hero.top + 92.0, hero.left + 330.0, hero.top + 132.0), theme::TEXT2, 13.0, 400, Align::Leading);
        let stage = rect(hero.left + 360.0, hero.top + 20.0, hero.right - 20.0, hero.bottom - 20.0);
        self.dock_stage(stage, c.dock_mag as f32);

        let first = hero.bottom + 20.0;
        let r = self.row(self.row_top(0, first), glyph::SPEED, "Animation duration", "How long one switch animation takes");
        self.slider(r.right - 20.0, (r.top + r.bottom) * 0.5, ((c.duration * 1000.0 - 80.0) / 520.0) as f32, &format!("{} ms", (c.duration * 1000.0).round() as i32), Slider::Duration);

        let r = self.row(self.row_top(1, first), glyph::VIEW_ALL, "Dock magnification", "How much the card under focus grows in Dock");
        self.slider(r.right - 20.0, (r.top + r.bottom) * 0.5, ((c.dock_mag - 1.2) / 1.4) as f32, &format!("{:.2}×", c.dock_mag), Slider::Magnification);

        let r = self.row(self.row_top(2, first), glyph::SETTINGS, "Follow Windows animation setting", "Skip transitions when Animation effects is off in Windows");
        self.toggle(r.right - 20.0, (r.top + r.bottom) * 0.5, c.respect_system_animations, Toggle::FollowWindowsAnimations);
    }

    fn page_gestures(&self) {
        let c = self.settings.read().clone();
        let mut top = Self::BODY_TOP;
        if self.touchpad_warning && !c.warn_dismissed {
            let bar = rect(self.content_left(), top, self.content_right(), top + 64.0);
            self.d2d.fill_rounded_rect(bar, 10.0, rgba(0.24, 0.19, 0.08, 1.0));
            self.d2d.draw_rounded_rect(bar, 10.0, rgba(1.0, 0.85, 0.45, 0.18), 1.0);
            self.d2d.icon(glyph::WARNING, rect(bar.left + 16.0, bar.top, bar.left + 44.0, bar.bottom), theme::AMBER, 16.0);
            self.text("Windows three-finger swipes are still on", rect(bar.left + 56.0, bar.top + 12.0, bar.right - 280.0, bar.top + 32.0), theme::TEXT1, 13.5, 600, Align::Leading);
            self.text("Set Touchpad › Three-finger gestures › Swipes to Nothing.", rect(bar.left + 56.0, bar.top + 32.0, bar.right - 280.0, bar.bottom - 10.0), theme::TEXT2, 12.5, 400, Align::Leading);
            let cy = (bar.top + bar.bottom) * 0.5;
            self.button(rect(bar.right - 262.0, cy - 17.0, bar.right - 112.0, cy + 17.0), "Open Touchpad", None, Hit::WarnOpen, false);
            self.button(rect(bar.right - 102.0, cy - 17.0, bar.right - 16.0, cy + 17.0), "Dismiss", None, Hit::WarnDismiss, false);
            top = bar.bottom + 16.0;
        }

        let r = self.row(self.row_top(0, top), glyph::TOUCH, "Swipe sensitivity", "Finger travel needed for one switch; lower is more sensitive");
        self.slider(r.right - 20.0, (r.top + r.bottom) * 0.5, ((c.sensitivity as f64 - 120.0) / 280.0) as f32, &c.sensitivity.to_string(), Slider::Sensitivity);

        let r = self.row(self.row_top(1, top), glyph::TOUCH, "Pinch gesture", "Pinch three fingers inward to send a shortcut");
        self.toggle(r.right - 20.0, (r.top + r.bottom) * 0.5, c.pinch_enabled, Toggle::Pinch);

        let r = self.row(self.row_top(2, top), glyph::KEYBOARD, "Pinch shortcut", "The key combination sent when you pinch");
        let cy = (r.top + r.bottom) * 0.5;
        let clear = rect(r.right - 92.0, cy - 17.0, r.right - 20.0, cy + 17.0);
        self.button(clear, "Clear", None, Hit::PinchClear, false);
        let record = rect(clear.left - 108.0, cy - 17.0, clear.left - 8.0, cy + 17.0);
        if self.capture_pinch {
            self.button(record, "Press keys…", None, Hit::PinchRecord, true);
        } else {
            self.button(record, "Record", None, Hit::PinchRecord, false);
        }
        self.keycaps(&shortcut_text(&c.pinch_mods, c.pinch_vk), record.left - 16.0, cy);

        let tips = rect(self.content_left(), self.row_top(3, top) + 8.0, self.content_right(), self.row_top(3, top) + 60.0);
        self.d2d.icon(glyph::INFO, rect(tips.left + 4.0, tips.top, tips.left + 24.0, tips.top + 20.0), theme::TEXT3, 13.0);
        self.text(
            "Swipe left or right with three fingers to switch. Hold still for a moment to open the picker, then glide to choose.",
            rect(tips.left + 32.0, tips.top, tips.right, tips.top + 20.0),
            theme::TEXT3,
            12.5,
            400,
            Align::Leading,
        );
    }

    fn page_keyboard(&self) {
        let c = self.settings.read().clone();
        let top = Self::BODY_TOP;
        let r = self.row(self.row_top(0, top), glyph::KEYBOARD, "Replace the Windows Alt+Tab switcher", "Alt+Tab opens AppSwitcher instead of the built-in switcher");
        self.toggle(r.right - 20.0, (r.top + r.bottom) * 0.5, c.alt_tab, Toggle::AltTab);

        let r = self.row(self.row_top(1, top), glyph::KEYBOARD, "Switch shortcut", "Hold the modifier and press the key to cycle");
        let cy = (r.top + r.bottom) * 0.5;
        let key = rect(r.right - 140.0, cy - 17.0, r.right - 20.0, cy + 17.0);
        let modifier = rect(key.left - 150.0, cy - 17.0, key.left - 30.0, cy + 17.0);
        let mod_label = if c.hotkey_mod.eq_ignore_ascii_case("ctrl") { "Ctrl" } else { "Alt" };
        self.combo(modifier, mod_label, Combo::HotkeyMod);
        self.text("+", rect(modifier.right, cy - 10.0, key.left, cy + 10.0), theme::TEXT3, 14.0, 400, Align::Center);
        self.combo(key, key_name(c.hotkey_key), Combo::HotkeyKey);

        let r = self.row(self.row_top(2, top), glyph::VIEW_ALL, "Windows of the same app", "Cycle between windows of the app you're in");
        self.keycaps("Alt + `", r.right - 20.0, (r.top + r.bottom) * 0.5);

        // Picker cheat sheet
        let sheet = rect(self.content_left(), self.row_top(3, top), self.content_right(), self.row_top(3, top) + 150.0);
        self.d2d.fill_rounded_rect(sheet, 10.0, theme::CARD);
        self.d2d.draw_rounded_rect(sheet, 10.0, theme::STROKE, 1.0);
        self.text("In the picker", rect(sheet.left + 20.0, sheet.top + 14.0, sheet.right, sheet.top + 36.0), theme::TEXT1, 14.0, 600, Align::Leading);
        let items = [
            ("← →  ·  wheel", "Move the highlight"),
            ("1 – 9", "Switch to that window"),
            ("Q  ·  middle-click", "Close a window"),
            ("Esc", "Cancel"),
        ];
        let col_w = (sheet.right - sheet.left - 40.0) / 2.0;
        for (i, (keys, what)) in items.iter().enumerate() {
            let x = sheet.left + 20.0 + (i % 2) as f32 * col_w;
            let y = sheet.top + 50.0 + (i / 2) as f32 * 44.0;
            let cap_w = self.d2d.measure(keys, 12.5, 600, Font::Text) + 22.0;
            let cap = rect(x, y, x + cap_w, y + 28.0);
            self.d2d.fill_rounded_rect(cap, 6.0, rgba(1.0, 1.0, 1.0, 0.07));
            self.d2d.draw_rounded_rect(cap, 6.0, rgba(1.0, 1.0, 1.0, 0.10), 1.0);
            self.text(keys, cap, theme::TEXT1, 12.5, 600, Align::Center);
            self.text(what, rect(cap.right + 14.0, y, x + col_w - 10.0, y + 28.0), theme::TEXT2, 13.0, 400, Align::Leading);
        }
    }

    fn page_hidden(&self) {
        let blocklist = self.settings.read().blocklist.clone();
        let ac = self.accent();
        // Header actions: count + refresh
        let right = self.content_right();
        // Kept clear of the close button in the top-right corner.
        let refresh = rect(right - 170.0, 48.0, right - 50.0, 82.0);
        self.button(refresh, "Refresh", Some(glyph::REFRESH), Hit::Refresh, false);
        let hidden_count = self.open_titles.iter().filter(|t| blocklist.contains(t)).count();
        self.text(
            &format!("{} windows · {} hidden", self.open_titles.len(), hidden_count),
            rect(right - 470.0, 48.0, refresh.left - 16.0, 82.0),
            theme::TEXT3,
            12.5,
            400,
            Align::Trailing,
        );

        let list_top = Self::BODY_TOP;
        let row_h = 52.0;
        let start = self.window_page * WINDOWS_PER_PAGE;
        let shown: Vec<(usize, &String)> = self.open_titles.iter().enumerate().skip(start).take(WINDOWS_PER_PAGE).collect();
        if shown.is_empty() {
            let empty = rect(self.content_left(), list_top, right, list_top + 120.0);
            self.d2d.fill_rounded_rect(empty, 10.0, theme::CARD);
            self.d2d.draw_rounded_rect(empty, 10.0, theme::STROKE, 1.0);
            self.text("No windows to show right now", empty, theme::TEXT2, 13.5, 400, Align::Center);
            return;
        }
        let card = rect(self.content_left(), list_top, right, list_top + shown.len() as f32 * row_h);
        self.d2d.fill_rounded_rect(card, 10.0, theme::CARD);
        self.d2d.draw_rounded_rect(card, 10.0, theme::STROKE, 1.0);
        for (row, (index, title)) in shown.iter().enumerate() {
            let r = rect(card.left, card.top + row as f32 * row_h, card.right, card.top + (row + 1) as f32 * row_h);
            if row > 0 {
                self.d2d.fill_rect(rect(r.left + 16.0, r.top, r.right - 16.0, r.top + 1.0), theme::STROKE);
            }
            let hidden = blocklist.contains(title);
            let cy = (r.top + r.bottom) * 0.5;
            if self.hovered(r) {
                self.d2d.fill_rounded_rect(inset(r, 4.0, 4.0), 8.0, rgba(1.0, 1.0, 1.0, 0.03));
            }
            self.d2d.icon(
                if hidden { glyph::HIDE } else { glyph::VIEW },
                rect(r.left + 16.0, r.top, r.left + 44.0, r.bottom),
                if hidden { ac } else { theme::TEXT3 },
                14.0,
            );
            self.text(title, rect(r.left + 56.0, r.top, r.right - 150.0, r.bottom), if hidden { theme::TEXT3 } else { theme::TEXT1 }, 13.5, 400, Align::Leading);
            self.toggle(r.right - 20.0, cy, hidden, Toggle::AltTab);
            // Re-target the toggle's hit to this window.
            if let Some(last) = self.hits.borrow_mut().last_mut() {
                last.1 = Hit::HideWindow(*index);
            }
        }
        let pages = self.open_titles.len().div_ceil(WINDOWS_PER_PAGE);
        if pages > 1 {
            let y = card.bottom + 14.0;
            let prev = rect(self.content_left(), y, self.content_left() + 110.0, y + 34.0);
            let next = rect(right - 110.0, y, right, y + 34.0);
            self.button(prev, "Previous", Some(glyph::CHEVRON_LEFT), Hit::PagePrev, false);
            self.button(next, "Next", Some(glyph::CHEVRON_RIGHT), Hit::PageNext, false);
            self.text(&format!("Page {} of {}", self.window_page + 1, pages), rect(prev.right, y, next.left, y + 34.0), theme::TEXT2, 13.0, 400, Align::Center);
        }
    }

    fn page_general(&self) {
        let top = Self::BODY_TOP;
        let r = self.row(self.row_top(0, top), glyph::POWER, "Start with Windows", "Launch AppSwitcher automatically when you sign in");
        self.toggle(r.right - 20.0, (r.top + r.bottom) * 0.5, self.autostart, Toggle::Autostart);

        let r = self.row(self.row_top(1, top), glyph::FOLDER, "Settings file", "%APPDATA%\\AppSwitcher\\settings.json");
        let cy = (r.top + r.bottom) * 0.5;
        self.button(rect(r.right - 150.0, cy - 17.0, r.right - 20.0, cy + 17.0), "Open folder", Some(glyph::FOLDER), Hit::OpenConfigFolder, false);

        let r = self.row(self.row_top(2, top), glyph::INFO, "About", "Native Rust engine · Direct2D renderer");
        let cy = (r.top + r.bottom) * 0.5;
        self.text(&format!("Version {}", env!("CARGO_PKG_VERSION")), rect(r.right - 260.0, cy - 10.0, r.right - 20.0, cy + 10.0), theme::TEXT2, 13.0, 500, Align::Trailing);
    }

    fn draw_flyout(&self, w: f32, h: f32) {
        let (Some(open), Some(anchor)) = (self.open_combo, self.combo_anchor.get()) else {
            return;
        };
        let c = self.settings.read().clone();
        let (items, current): (Vec<&str>, usize) = match open {
            Combo::Animation => (
                ANIMATIONS.iter().map(|(_, v)| *v).collect(),
                ANIMATIONS.iter().position(|(k, _)| *k == c.animation).unwrap_or(0),
            ),
            Combo::Layout => (
                LAYOUTS.iter().map(|(_, v)| *v).collect(),
                LAYOUTS.iter().position(|(k, _)| *k == c.layout).unwrap_or(0),
            ),
            Combo::HotkeyMod => (vec!["Alt", "Ctrl"], usize::from(c.hotkey_mod.eq_ignore_ascii_case("ctrl"))),
            Combo::HotkeyKey => (
                HOTKEYS.iter().map(|(n, _)| *n).collect(),
                HOTKEYS.iter().position(|(_, k)| *k == c.hotkey_key).unwrap_or(0),
            ),
        };
        let ac = self.accent();
        // Click anywhere else closes the dropdown.
        self.hit(rect(0.0, 0.0, w, h), Hit::Dismiss);
        let item_h = 34.0;
        let menu_h = items.len() as f32 * item_h + 8.0;
        let below = anchor.bottom + 6.0 + menu_h <= h - 12.0;
        let top = if below { anchor.bottom + 6.0 } else { anchor.top - 6.0 - menu_h };
        let menu = rect(anchor.left, top, anchor.right, top + menu_h);
        self.d2d.fill_rounded_rect(rect(menu.left - 2.0, menu.top + 4.0, menu.right + 2.0, menu.bottom + 12.0), 12.0, rgba(0.0, 0.0, 0.0, 0.40));
        self.d2d.fill_rounded_rect(menu, 10.0, theme::FLYOUT);
        self.d2d.draw_rounded_rect(menu, 10.0, theme::CONTROL_STROKE, 1.0);
        for (i, item) in items.iter().enumerate() {
            let slot = rect(menu.left + 4.0, menu.top + 4.0 + i as f32 * item_h, menu.right - 4.0, menu.top + 4.0 + (i + 1) as f32 * item_h);
            let selected = i == current;
            if selected || self.hovered(slot) {
                self.d2d.fill_rounded_rect(slot, 6.0, rgba(1.0, 1.0, 1.0, if selected { 0.08 } else { 0.05 }));
            }
            if selected {
                let cy = (slot.top + slot.bottom) * 0.5;
                self.d2d.fill_rounded_rect(rect(slot.left, cy - 8.0, slot.left + 3.0, cy + 8.0), 1.5, ac);
                self.d2d.icon(glyph::CHECK, rect(slot.right - 30.0, slot.top, slot.right - 8.0, slot.bottom), ac, 11.0);
            }
            self.text(item, rect(slot.left + 14.0, slot.top, slot.right - 34.0, slot.bottom), theme::TEXT1, 13.5, if selected { 600 } else { 400 }, Align::Leading);
            self.hit(slot, Hit::ComboItem(open, i));
        }
    }
}

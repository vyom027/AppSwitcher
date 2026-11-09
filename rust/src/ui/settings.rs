use crate::config::Settings;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::render::d2d::D2DContext;
use crossbeam_channel::Sender;
use std::mem::size_of;
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, RedrawWindow, ScreenToClient, PAINTSTRUCT, RDW_INVALIDATE,
    RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, PostMessageW, RegisterClassExW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    GWLP_USERDATA, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE,
    WM_ERASEBKGND, WM_LBUTTONDOWN, WM_PAINT, WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU,
};

pub struct SettingsDialog {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
    pub d2d: D2DContext,
    pub settings: Arc<parking_lot::RwLock<Settings>>,
    pub event_tx: Sender<AppEvent>,
    pub main_hwnd: usize,
    pub active_tab: usize,
    pub status_msg: String,
}

unsafe impl Send for SettingsDialog {}
unsafe impl Sync for SettingsDialog {}

const ACCENT_PRESETS: &[(&str, [u8; 3])] = &[
    ("Cyber Blue", [0, 150, 255]),
    ("Apple Purple", [165, 95, 255]),
    ("Emerald Mint", [0, 215, 160]),
    ("Sunset Orange", [255, 120, 50]),
    ("Electric Coral", [255, 75, 130]),
    ("Golden Sun", [255, 195, 30]),
    ("Ice Silver", [210, 225, 250]),
    ("Neon Violet", [195, 70, 255]),
];

const LAYOUT_NAMES: &[(&str, &str)] = &[
    ("dock", "macOS-style Magnified Dock + Large Preview"),
    ("row", "Smooth Centered Horizontal Carousel"),
    ("grid", "2D Responsive Window Matrix"),
    ("coverflow", "3D Angled Perspective Shelf"),
    ("hero", "Big Stage with Bottom Filmstrip"),
    ("wallet", "Vertical Overlapping Card Deck"),
    ("fan", "Curved Radial Card Arc"),
];

const ANIM_NAMES: &[&str] = &[
    "slide", "duo_fold", "fade", "zoom",
    "cube", "flip", "push", "stack",
];

unsafe extern "system" fn settings_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
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
            WM_ERASEBKGND => {
                return LRESULT(1);
            }
            WM_LBUTTONDOWN => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(hwnd, &mut pt);

                let dpi = windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd).max(96) as f32;
                let scale = dpi / 96.0;

                let x = pt.x as f32 / scale;
                let y = pt.y as f32 / scale;

                dlg.handle_click(x, y);
                dlg.render();
                let _ = RedrawWindow(hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
                return LRESULT(0);
            }
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
                return LRESULT(0);
            }
            _ => {}
        }
    }
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

impl SettingsDialog {
    pub fn new(
        settings: Arc<parking_lot::RwLock<Settings>>,
        event_tx: Sender<AppEvent>,
        main_hwnd: usize,
    ) -> Result<Box<Self>, windows::core::Error> {
        let desired_w = 860;
        let desired_h = 580;

        unsafe {
            let h_instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherSettingsWndClass");

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(settings_wnd_proc);
            wc.hInstance = h_instance.into();
            wc.lpszClassName = class_name;

            let _ = RegisterClassExW(&wc);

            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

            let mut rc = RECT {
                left: 0,
                top: 0,
                right: desired_w as i32,
                bottom: desired_h as i32,
            };
            let _ = AdjustWindowRectEx(&mut rc, style, false, WINDOW_EX_STYLE(0));
            let win_w = rc.right - rc.left;
            let win_h = rc.bottom - rc.top;

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("AppSwitcher — Modern Settings"),
                style,
                120,
                120,
                win_w,
                win_h,
                HWND(std::ptr::null_mut()),
                None,
                h_instance,
                None,
            )?;

            let mut client_rc = RECT::default();
            let _ = GetClientRect(hwnd, &mut client_rc);
            let width = (client_rc.right - client_rc.left).max(800) as u32;
            let height = (client_rc.bottom - client_rc.top).max(520) as u32;

            let d2d = D2DContext::new(hwnd, width, height)?;

            let mut dlg = Box::new(Self {
                hwnd,
                width,
                height,
                d2d,
                settings,
                event_tx,
                main_hwnd,
                active_tab: 0,
                status_msg: String::new(),
            });

            SetWindowLongPtrW(hwnd, GWLP_USERDATA, dlg.as_mut() as *mut _ as isize);

            Ok(dlg)
        }
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetWindowPos(self.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = RedrawWindow(self.hwnd, None, None, RDW_INVALIDATE | RDW_UPDATENOW);
        }
        self.render();
    }

    fn handle_click(&mut self, x: f32, y: f32) {
        // 1. Sidebar tab clicks
        if x < 210.0 && y >= 60.0 {
            let tab = ((y - 60.0) / 48.0) as usize;
            if tab < 5 {
                self.active_tab = tab;
                self.status_msg.clear();
            }
            return;
        }

        // Bottom Action Buttons
        let btn_y = self.height as f32 - 58.0;

        // Preview button: x 440..600
        if y >= btn_y - 6.0 && y <= self.height as f32 && x >= 440.0 && x <= 600.0 {
            if self.active_tab == 0 {
                let _ = self.event_tx.send(AppEvent::Swipe(1));
            } else {
                let _ = self.event_tx.send(AppEvent::AltTab { direction: 1 });
            }
            unsafe {
                let _ = PostMessageW(
                    HWND(self.main_hwnd as *mut _),
                    WM_APP_EVENT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            self.status_msg = "Testing preview...".to_string();
            return;
        }

        // Save button: x >= width - 170
        if y >= btn_y - 6.0 && y <= self.height as f32 && x >= self.width as f32 - 170.0 {
            let cfg = self.settings.read().clone();
            if cfg.save().is_ok() {
                self.status_msg = "Saved successfully!".to_string();
            } else {
                self.status_msg = "Error saving settings.".to_string();
            }
            return;
        }

        // Tab-specific interactions inside the right card
        match self.active_tab {
            0 => {
                // Motion & Speed Tab:
                // Animation selection: 2 rows of 4 pills at y = 135..215
                if y >= 135.0 && y <= 215.0 && x >= 250.0 {
                    let row = if y >= 175.0 { 1 } else { 0 };
                    let col = ((x - 250.0) / 135.0) as usize;
                    let idx = row * 4 + col;
                    if idx < ANIM_NAMES.len() && col < 4 {
                        let selected_anim = ANIM_NAMES[idx].to_string();
                        self.settings.write().animation = selected_anim.clone();
                        let _ = self.settings.read().save();
                        self.status_msg = format!("Animation set to {}", selected_anim);
                    }
                }
                // Duration presets: y = 255..290
                if y >= 255.0 && y <= 290.0 && x >= 250.0 {
                    let presets = [0.14, 0.22, 0.32, 0.45];
                    let col = ((x - 250.0) / 125.0) as usize;
                    if col < presets.len() {
                        self.settings.write().duration = presets[col];
                        let _ = self.settings.read().save();
                        self.status_msg = format!("Duration set to {}s", presets[col]);
                    }
                }
                // Sensitivity presets: y = 340..375
                if y >= 340.0 && y <= 375.0 && x >= 250.0 {
                    let sens = [160, 220, 300];
                    let col = ((x - 250.0) / 135.0) as usize;
                    if col < sens.len() {
                        self.settings.write().sensitivity = sens[col];
                        let _ = self.settings.read().save();
                        self.status_msg = format!("Sensitivity set to {}px", sens[col]);
                    }
                }
            }
            1 => {
                // Layouts Tab: 7 cards in 2 columns
                if x >= 250.0 && x <= 790.0 && y >= 130.0 {
                    let row = ((y - 130.0) / 52.0) as usize;
                    if row < LAYOUT_NAMES.len() {
                        let selected_layout = LAYOUT_NAMES[row].0.to_string();
                        self.settings.write().layout = selected_layout.clone();
                        let _ = self.settings.read().save();
                        self.status_msg = format!("Layout set to {}", selected_layout);
                    }
                }
            }
            2 => {
                // Hotkeys Tab:
                // Alt vs Ctrl toggle: y = 150..185
                if y >= 150.0 && y <= 185.0 && x >= 250.0 && x <= 460.0 {
                    let is_ctrl = x >= 355.0;
                    self.settings.write().hotkey_mod = if is_ctrl { "ctrl".to_string() } else { "alt".to_string() };
                    let _ = self.settings.read().save();
                    self.status_msg = format!("Hotkey set to {}", if is_ctrl { "Ctrl+Tab" } else { "Alt+Tab" });
                }
                // Pinch toggle: y = 240..275
                if y >= 240.0 && y <= 275.0 && x >= 250.0 && x <= 420.0 {
                    let cur = self.settings.read().pinch_enabled;
                    self.settings.write().pinch_enabled = !cur;
                    let _ = self.settings.read().save();
                    self.status_msg = format!("Pinch gesture {}", if !cur { "enabled" } else { "disabled" });
                }
            }
            3 => {
                // Appearance Tab: 8 Accent Color Circles at y = 160..210
                if y >= 150.0 && y <= 210.0 && x >= 250.0 {
                    let idx = ((x - 250.0) / 65.0) as usize;
                    if idx < ACCENT_PRESETS.len() {
                        self.settings.write().accent = ACCENT_PRESETS[idx].1;
                        let _ = self.settings.read().save();
                        self.status_msg = format!("Theme set to {}", ACCENT_PRESETS[idx].0);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn render(&self) {
        self.d2d.begin_draw();
        // Deep modern dark background (#0f1118)
        self.d2d.clear(0.06, 0.07, 0.09, 1.0);

        let cfg = self.settings.read();
        let accent = D2D1_COLOR_F {
            r: cfg.accent[0] as f32 / 255.0,
            g: cfg.accent[1] as f32 / 255.0,
            b: cfg.accent[2] as f32 / 255.0,
            a: 1.0,
        };

        // 1. Sidebar tabs (Left 210px)
        let sidebar_rect = D2D_RECT_F {
            left: 0.0,
            top: 0.0,
            right: 210.0,
            bottom: self.height as f32,
        };
        self.d2d.fill_rounded_rect(
            sidebar_rect,
            0.0,
            D2D1_COLOR_F {
                r: 0.05,
                g: 0.06,
                b: 0.08,
                a: 1.0,
            },
        );

        // App Logo / Title
        let logo_rect = D2D_RECT_F {
            left: 20.0,
            top: 20.0,
            right: 200.0,
            bottom: 48.0,
        };
        self.d2d.draw_text(
            "AppSwitcher 2.0",
            logo_rect,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            16.0,
            true,
            false,
        );

        let tabs = ["Motion & Speed", "Layouts", "Hotkeys", "Appearance", "Windows"];
        for (i, &name) in tabs.iter().enumerate() {
            let tab_y = 60.0 + i as f32 * 48.0;
            let tab_rect = D2D_RECT_F {
                left: 14.0,
                top: tab_y,
                right: 196.0,
                bottom: tab_y + 40.0,
            };

            if i == self.active_tab {
                self.d2d.fill_rounded_rect(tab_rect, 10.0, accent);
                self.d2d.draw_text(
                    name,
                    tab_rect,
                    D2D1_COLOR_F {
                        r: 0.04,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    },
                    13.5,
                    true,
                    true,
                );
            } else {
                self.d2d.draw_text(
                    name,
                    tab_rect,
                    D2D1_COLOR_F {
                        r: 0.70,
                        g: 0.75,
                        b: 0.85,
                        a: 1.0,
                    },
                    13.5,
                    false,
                    true,
                );
            }
        }

        // 2. Main Content Card (Right pane)
        let content_rect = D2D_RECT_F {
            left: 230.0,
            top: 20.0,
            right: self.width as f32 - 20.0,
            bottom: self.height as f32 - 68.0,
        };
        self.d2d.fill_rounded_rect(
            content_rect,
            16.0,
            D2D1_COLOR_F {
                r: 0.08,
                g: 0.10,
                b: 0.13,
                a: 0.90,
            },
        );
        self.d2d.draw_rounded_rect(
            content_rect,
            16.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.06,
            },
            1.0,
        );

        // Header
        let header_rect = D2D_RECT_F {
            left: 254.0,
            top: 38.0,
            right: 700.0,
            bottom: 68.0,
        };
        self.d2d.draw_text(
            tabs[self.active_tab],
            header_rect,
            D2D1_COLOR_F {
                r: 0.96,
                g: 0.97,
                b: 1.0,
                a: 1.0,
            },
            20.0,
            true,
            false,
        );

        // Tab Content
        match self.active_tab {
            0 => {
                // Section: Switch Animation
                self.d2d.draw_text(
                    "Switch Transition Animation",
                    D2D_RECT_F {
                        left: 254.0,
                        top: 110.0,
                        right: 700.0,
                        bottom: 135.0,
                    },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                for (idx, &anim) in ANIM_NAMES.iter().enumerate() {
                    let row = idx / 4;
                    let col = idx % 4;
                    let ax = 250.0 + col as f32 * 135.0;
                    let ay = 138.0 + row as f32 * 40.0;
                    let pill_rect = D2D_RECT_F {
                        left: ax,
                        top: ay,
                        right: ax + 125.0,
                        bottom: ay + 34.0,
                    };
                    let is_sel = cfg.animation.replace('_', " ").eq_ignore_ascii_case(&anim.replace('_', " "));
                    if is_sel {
                        self.d2d.fill_rounded_rect(pill_rect, 8.0, accent);
                        self.d2d.draw_text(
                            anim,
                            pill_rect,
                            D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 },
                            13.0,
                            true,
                            true,
                        );
                    } else {
                        self.d2d.fill_rounded_rect(
                            pill_rect,
                            8.0,
                            D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 },
                        );
                        self.d2d.draw_text(
                            anim,
                            pill_rect,
                            D2D1_COLOR_F { r: 0.75, g: 0.80, b: 0.90, a: 1.0 },
                            13.0,
                            false,
                            true,
                        );
                    }
                }

                // Section: Duration
                self.d2d.draw_text(
                    "Transition Speed",
                    D2D_RECT_F {
                        left: 254.0,
                        top: 228.0,
                        right: 700.0,
                        bottom: 250.0,
                    },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                let dur_presets = [("Snappy (140ms)", 0.14), ("Balanced (220ms)", 0.22), ("Smooth (320ms)", 0.32), ("Cinematic (450ms)", 0.45)];
                for (col, &(label, val)) in dur_presets.iter().enumerate() {
                    let dx = 250.0 + col as f32 * 125.0;
                    let rect = D2D_RECT_F {
                        left: dx,
                        top: 255.0,
                        right: dx + 118.0,
                        bottom: 290.0,
                    };
                    let is_sel = (cfg.duration - val).abs() < 0.03;
                    if is_sel {
                        self.d2d.fill_rounded_rect(rect, 8.0, accent);
                        self.d2d.draw_text(
                            label,
                            rect,
                            D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 },
                            11.5,
                            true,
                            true,
                        );
                    } else {
                        self.d2d.fill_rounded_rect(
                            rect,
                            8.0,
                            D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 },
                        );
                        self.d2d.draw_text(
                            label,
                            rect,
                            D2D1_COLOR_F { r: 0.75, g: 0.80, b: 0.90, a: 1.0 },
                            11.5,
                            false,
                            true,
                        );
                    }
                }

                // Section: Sensitivity
                self.d2d.draw_text(
                    "Touchpad Glide Sensitivity",
                    D2D_RECT_F {
                        left: 254.0,
                        top: 312.0,
                        right: 700.0,
                        bottom: 334.0,
                    },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                let sens_presets = [("High (160)", 160), ("Balanced (220)", 220), ("Relaxed (300)", 300)];
                for (col, &(label, val)) in sens_presets.iter().enumerate() {
                    let sx = 250.0 + col as f32 * 135.0;
                    let rect = D2D_RECT_F {
                        left: sx,
                        top: 340.0,
                        right: sx + 125.0,
                        bottom: 375.0,
                    };
                    let is_sel = cfg.sensitivity == val;
                    if is_sel {
                        self.d2d.fill_rounded_rect(rect, 8.0, accent);
                        self.d2d.draw_text(
                            label,
                            rect,
                            D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 },
                            12.0,
                            true,
                            true,
                        );
                    } else {
                        self.d2d.fill_rounded_rect(
                            rect,
                            8.0,
                            D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 },
                        );
                        self.d2d.draw_text(
                            label,
                            rect,
                            D2D1_COLOR_F { r: 0.75, g: 0.80, b: 0.90, a: 1.0 },
                            12.0,
                            false,
                            true,
                        );
                    }
                }
            }
            1 => {
                // Layouts Tab: 7 selectable layout cards
                self.d2d.draw_text(
                    "Click a layout to set as active window picker presentation:",
                    D2D_RECT_F {
                        left: 254.0,
                        top: 85.0,
                        right: 780.0,
                        bottom: 115.0,
                    },
                    D2D1_COLOR_F { r: 0.70, g: 0.75, b: 0.85, a: 1.0 },
                    13.0,
                    false,
                    false,
                );

                for (i, &(name, desc)) in LAYOUT_NAMES.iter().enumerate() {
                    let y = 130.0 + i as f32 * 52.0;
                    let card_rect = D2D_RECT_F {
                        left: 250.0,
                        top: y,
                        right: 800.0,
                        bottom: y + 44.0,
                    };
                    let is_sel = cfg.layout.eq_ignore_ascii_case(name);

                    if is_sel {
                        self.d2d.fill_rounded_rect(
                            card_rect,
                            8.0,
                            D2D1_COLOR_F {
                                r: accent.r * 0.25,
                                g: accent.g * 0.25,
                                b: accent.b * 0.25,
                                a: 0.8,
                            },
                        );
                        self.d2d.draw_rounded_rect(card_rect, 8.0, accent, 2.0);
                    } else {
                        self.d2d.fill_rounded_rect(
                            card_rect,
                            8.0,
                            D2D1_COLOR_F { r: 0.11, g: 0.13, b: 0.17, a: 0.7 },
                        );
                        self.d2d.draw_rounded_rect(
                            card_rect,
                            8.0,
                            D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.08 },
                            1.0,
                        );
                    }

                    // Layout Name
                    self.d2d.draw_text(
                        &name.to_uppercase(),
                        D2D_RECT_F {
                            left: 266.0,
                            top: y + 12.0,
                            right: 370.0,
                            bottom: y + 36.0,
                        },
                        if is_sel { accent } else { D2D1_COLOR_F { r: 0.9, g: 0.9, b: 0.95, a: 1.0 } },
                        14.0,
                        true,
                        false,
                    );

                    // Layout Description
                    self.d2d.draw_text(
                        desc,
                        D2D_RECT_F {
                            left: 380.0,
                            top: y + 14.0,
                            right: 790.0,
                            bottom: y + 36.0,
                        },
                        D2D1_COLOR_F { r: 0.65, g: 0.70, b: 0.80, a: 1.0 },
                        12.5,
                        false,
                        false,
                    );
                }
            }
            2 => {
                // Hotkeys Tab
                self.d2d.draw_text(
                    "Keyboard Switcher Trigger Chord",
                    D2D_RECT_F { left: 254.0, top: 110.0, right: 700.0, bottom: 135.0 },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                let is_ctrl = cfg.hotkey_mod.eq_ignore_ascii_case("ctrl");
                let alt_rect = D2D_RECT_F { left: 250.0, top: 150.0, right: 345.0, bottom: 185.0 };
                let ctrl_rect = D2D_RECT_F { left: 355.0, top: 150.0, right: 450.0, bottom: 185.0 };

                if !is_ctrl {
                    self.d2d.fill_rounded_rect(alt_rect, 8.0, accent);
                    self.d2d.draw_text("Alt + Tab", alt_rect, D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 }, 13.0, true, true);
                    self.d2d.fill_rounded_rect(ctrl_rect, 8.0, D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 });
                    self.d2d.draw_text("Ctrl + Tab", ctrl_rect, D2D1_COLOR_F { r: 0.7, g: 0.75, b: 0.85, a: 1.0 }, 13.0, false, true);
                } else {
                    self.d2d.fill_rounded_rect(alt_rect, 8.0, D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 });
                    self.d2d.draw_text("Alt + Tab", alt_rect, D2D1_COLOR_F { r: 0.7, g: 0.75, b: 0.85, a: 1.0 }, 13.0, false, true);
                    self.d2d.fill_rounded_rect(ctrl_rect, 8.0, accent);
                    self.d2d.draw_text("Ctrl + Tab", ctrl_rect, D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 }, 13.0, true, true);
                }

                // 3-Finger Pinch
                self.d2d.draw_text(
                    "3-Finger Pinch Inward Action",
                    D2D_RECT_F { left: 254.0, top: 210.0, right: 700.0, bottom: 235.0 },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                let on_rect = D2D_RECT_F { left: 250.0, top: 240.0, right: 325.0, bottom: 275.0 };
                let off_rect = D2D_RECT_F { left: 335.0, top: 240.0, right: 410.0, bottom: 275.0 };
                if cfg.pinch_enabled {
                    self.d2d.fill_rounded_rect(on_rect, 8.0, accent);
                    self.d2d.draw_text("ON", on_rect, D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 }, 13.0, true, true);
                    self.d2d.fill_rounded_rect(off_rect, 8.0, D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 });
                    self.d2d.draw_text("OFF", off_rect, D2D1_COLOR_F { r: 0.7, g: 0.75, b: 0.85, a: 1.0 }, 13.0, false, true);
                } else {
                    self.d2d.fill_rounded_rect(on_rect, 8.0, D2D1_COLOR_F { r: 0.12, g: 0.14, b: 0.18, a: 0.8 });
                    self.d2d.draw_text("ON", on_rect, D2D1_COLOR_F { r: 0.7, g: 0.75, b: 0.85, a: 1.0 }, 13.0, false, true);
                    self.d2d.fill_rounded_rect(off_rect, 8.0, accent);
                    self.d2d.draw_text("OFF", off_rect, D2D1_COLOR_F { r: 0.04, g: 0.05, b: 0.07, a: 1.0 }, 13.0, true, true);
                }
            }
            3 => {
                // Appearance Tab: 8 Accent Swatches
                self.d2d.draw_text(
                    "Select Accent Color Theme",
                    D2D_RECT_F { left: 254.0, top: 110.0, right: 700.0, bottom: 135.0 },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                for (idx, &(name, rgb)) in ACCENT_PRESETS.iter().enumerate() {
                    let cx = 250.0 + idx as f32 * 65.0;
                    let swatch_rect = D2D_RECT_F {
                        left: cx,
                        top: 150.0,
                        right: cx + 48.0,
                        bottom: 198.0,
                    };
                    let color = D2D1_COLOR_F {
                        r: rgb[0] as f32 / 255.0,
                        g: rgb[1] as f32 / 255.0,
                        b: rgb[2] as f32 / 255.0,
                        a: 1.0,
                    };
                    self.d2d.fill_rounded_rect(swatch_rect, 24.0, color);

                    let is_active = cfg.accent == rgb;
                    if is_active {
                        self.d2d.draw_rounded_rect(
                            D2D_RECT_F {
                                left: cx - 4.0,
                                top: 146.0,
                                right: cx + 52.0,
                                bottom: 202.0,
                            },
                            26.0,
                            D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.9 },
                            2.5,
                        );
                    }

                    // Mini label
                    self.d2d.draw_text(
                        name.split_whitespace().next().unwrap_or(""),
                        D2D_RECT_F {
                            left: cx - 6.0,
                            top: 206.0,
                            right: cx + 54.0,
                            bottom: 226.0,
                        },
                        D2D1_COLOR_F { r: 0.70, g: 0.75, b: 0.85, a: 1.0 },
                        10.5,
                        false,
                        true,
                    );
                }

                self.d2d.draw_text(
                    "Backdrop Blur: 3-pass fast Gaussian hardware simulation (0.8ms)\nAntialiasing: Direct2D Per-Primitive Hardware Multisampling\nWindow Borders: Apple-style continuous rounded corners (Squircle)",
                    D2D_RECT_F { left: 254.0, top: 260.0, right: 780.0, bottom: 350.0 },
                    D2D1_COLOR_F { r: 0.65, g: 0.70, b: 0.80, a: 1.0 },
                    13.0,
                    false,
                    false,
                );
            }
            _ => {
                // Windows Blocklist
                self.d2d.draw_text(
                    "Application Exclusions & Window Filtering",
                    D2D_RECT_F { left: 254.0, top: 110.0, right: 700.0, bottom: 135.0 },
                    D2D1_COLOR_F { r: 0.85, g: 0.88, b: 0.95, a: 1.0 },
                    14.0,
                    true,
                    false,
                );

                let info = format!(
                    "Excluded Applications Count: {}\n\nTip: You can middle-click any card in the switcher to instantly close that window!\n\nFiltered titles: {:?}",
                    cfg.blocklist.len(),
                    cfg.blocklist
                );
                self.d2d.draw_text(
                    &info,
                    D2D_RECT_F { left: 254.0, top: 150.0, right: 780.0, bottom: 350.0 },
                    D2D1_COLOR_F { r: 0.75, g: 0.80, b: 0.90, a: 1.0 },
                    13.5,
                    false,
                    false,
                );
            }
        }

        // Status message
        if !self.status_msg.is_empty() {
            self.d2d.draw_text(
                &self.status_msg,
                D2D_RECT_F {
                    left: 254.0,
                    top: self.height as f32 - 50.0,
                    right: 440.0,
                    bottom: self.height as f32 - 20.0,
                },
                accent,
                13.0,
                true,
                false,
            );
        }

        // Preview Button
        let prev_rect = D2D_RECT_F {
            left: 460.0,
            top: self.height as f32 - 56.0,
            right: 580.0,
            bottom: self.height as f32 - 20.0,
        };
        self.d2d.fill_rounded_rect(
            prev_rect,
            8.0,
            D2D1_COLOR_F { r: 0.14, g: 0.16, b: 0.22, a: 0.9 },
        );
        self.d2d.draw_rounded_rect(
            prev_rect,
            8.0,
            D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 0.15 },
            1.0,
        );
        let preview_label = if self.active_tab == 0 { "Test Anim" } else { "Test Layout" };
        self.d2d.draw_text(
            preview_label,
            prev_rect,
            D2D1_COLOR_F { r: 0.90, g: 0.92, b: 0.98, a: 1.0 },
            13.0,
            true,
            true,
        );

        // Save Button (Bottom-right)
        let save_rect = D2D_RECT_F {
            left: self.width as f32 - 150.0,
            top: self.height as f32 - 56.0,
            right: self.width as f32 - 24.0,
            bottom: self.height as f32 - 20.0,
        };
        self.d2d.fill_rounded_rect(save_rect, 8.0, accent);
        self.d2d.draw_text(
            "Save Changes",
            save_rect,
            D2D1_COLOR_F {
                r: 0.04,
                g: 0.05,
                b: 0.07,
                a: 1.0,
            },
            13.5,
            true,
            true,
        );

        let _ = self.d2d.end_draw();
    }
}

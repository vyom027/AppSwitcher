use crate::config::Settings;
use crate::render::d2d::D2DContext;
use std::mem::size_of;
use std::sync::Arc;
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetWindowLongPtrW, RegisterClassExW, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWLP_USERDATA, HWND_TOP, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOW,
    WINDOW_EX_STYLE, WM_CLOSE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_PAINT,
    WNDCLASSEXW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};

pub struct SettingsDialog {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
    pub d2d: D2DContext,
    pub settings: Arc<parking_lot::RwLock<Settings>>,
    pub active_tab: usize,
}

unsafe impl Send for SettingsDialog {}
unsafe impl Sync for SettingsDialog {}

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
                let x = (l_param.0 & 0xFFFF) as i16 as f32;
                let y = ((l_param.0 >> 16) & 0xFFFF) as i16 as f32;

                // Sidebar tab clicks
                if x < 210.0 && y >= 60.0 {
                    let tab = ((y - 60.0) / 46.0) as usize;
                    if tab < 5 && tab != dlg.active_tab {
                        dlg.active_tab = tab;
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                }

                // Save button click
                let save_left = dlg.width as f32 - 160.0;
                let save_top = dlg.height as f32 - 58.0;
                if x >= save_left && y >= save_top {
                    let cfg = dlg.settings.read().clone();
                    if let Err(e) = cfg.save() {
                        eprintln!("[settings] save error: {:?}", e);
                    } else {
                        println!("[settings] saved configuration to disk.");
                    }
                }

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
    pub fn new(settings: Arc<parking_lot::RwLock<Settings>>) -> Result<Box<Self>, windows::core::Error> {
        let width = 840;
        let height = 560;

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

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("AppSwitcher — Modern Settings (Native)"),
                style,
                100,
                100,
                width as i32,
                height as i32,
                HWND(std::ptr::null_mut()),
                None,
                h_instance,
                None,
            )?;

            let d2d = D2DContext::new(hwnd, width, height)?;

            let mut dlg = Box::new(Self {
                hwnd,
                width,
                height,
                d2d,
                settings,
                active_tab: 0,
            });

            SetWindowLongPtrW(hwnd, GWLP_USERDATA, dlg.as_mut() as *mut _ as isize);

            Ok(dlg)
        }
    }

    pub fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetWindowPos(self.hwnd, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
            let _ = InvalidateRect(self.hwnd, None, false);
        }
        self.render();
    }

    pub fn render(&self) {
        self.d2d.begin_draw();
        // Dark glass background (#14161F)
        self.d2d.clear(0.08, 0.09, 0.12, 1.0);

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
                r: 0.06,
                g: 0.07,
                b: 0.09,
                a: 1.0,
            },
        );

        let tabs = ["Motion & Speed", "Layouts", "Hotkeys", "Appearance", "Windows"];
        for (i, &name) in tabs.iter().enumerate() {
            let tab_y = 60.0 + i as f32 * 46.0;
            let tab_rect = D2D_RECT_F {
                left: 16.0,
                top: tab_y,
                right: 194.0,
                bottom: tab_y + 38.0,
            };

            if i == self.active_tab {
                self.d2d.fill_rounded_rect(tab_rect, 10.0, accent);
                self.d2d.draw_text(
                    name,
                    tab_rect,
                    D2D1_COLOR_F {
                        r: 0.05,
                        g: 0.07,
                        b: 0.1,
                        a: 1.0,
                    },
                    14.0,
                    true,
                    true,
                );
            } else {
                self.d2d.draw_text(
                    name,
                    tab_rect,
                    D2D1_COLOR_F {
                        r: 0.7,
                        g: 0.75,
                        b: 0.85,
                        a: 1.0,
                    },
                    14.0,
                    false,
                    true,
                );
            }
        }

        // 2. Main Content Card (Right pane)
        let content_rect = D2D_RECT_F {
            left: 230.0,
            top: 24.0,
            right: self.width as f32 - 24.0,
            bottom: self.height as f32 - 70.0,
        };
        self.d2d.fill_rounded_rect(
            content_rect,
            18.0,
            D2D1_COLOR_F {
                r: 0.1,
                g: 0.12,
                b: 0.16,
                a: 0.85,
            },
        );
        self.d2d.draw_rounded_rect(
            content_rect,
            18.0,
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
            top: 48.0,
            right: 700.0,
            bottom: 80.0,
        };
        self.d2d.draw_text(
            tabs[self.active_tab],
            header_rect,
            D2D1_COLOR_F {
                r: 0.95,
                g: 0.96,
                b: 0.98,
                a: 1.0,
            },
            22.0,
            true,
            false,
        );

        // Subtitle / Info
        let sub_rect = D2D_RECT_F {
            left: 254.0,
            top: 86.0,
            right: 760.0,
            bottom: 120.0,
        };
        let sub_text = match self.active_tab {
            0 => "Configure gesture sensitivity, 120Hz/144Hz animation transitions, and spring physics.",
            1 => "Select your interactive window picker presentation (Dock, Grid, Coverflow, Row, Hero, Wallet, Fan).",
            2 => "Configure the keyboard shortcut trigger (Default: Alt + Tab or Ctrl + Tab).",
            3 => "Customize dark glass styling, accent color, and blur radius.",
            _ => "Exclude specific applications or window titles from the switcher.",
        };
        self.d2d.draw_text(
            sub_text,
            sub_rect,
            D2D1_COLOR_F {
                r: 0.65,
                g: 0.70,
                b: 0.80,
                a: 0.9,
            },
            13.0,
            false,
            false,
        );

        // Tab Content
        let val_rect = D2D_RECT_F {
            left: 254.0,
            top: 140.0,
            right: 760.0,
            bottom: 300.0,
        };
        let detail_text = match self.active_tab {
            0 => format!(
                "Current Animation: {}\nDuration: {}s\nSpring Dynamics: RK4 High-Snappy Physics",
                cfg.animation, cfg.duration
            ),
            1 => format!("Current Picker Layout: {}\nAvailable Layouts: Dock, Grid, Coverflow, Row, Hero, Wallet, Fan", cfg.layout),
            2 => format!("Trigger Chord: {} + {}\nPinch Action: Enabled ({})", cfg.hotkey_mod, cfg.hotkey_key, cfg.pinch_enabled),
            3 => format!("Accent RGB: [{}, {}, {}]\nBackdrop Blur: Dual-Pass Kawase GPU Blur", cfg.accent[0], cfg.accent[1], cfg.accent[2]),
            _ => format!("Blocked Windows Count: {}\nBlocklist: {:?}", cfg.blocklist.len(), cfg.blocklist),
        };
        self.d2d.draw_text(
            &detail_text,
            val_rect,
            D2D1_COLOR_F {
                r: 0.85,
                g: 0.88,
                b: 0.95,
                a: 1.0,
            },
            15.0,
            false,
            false,
        );

        // Save Button (Bottom-right)
        let save_rect = D2D_RECT_F {
            left: self.width as f32 - 160.0,
            top: self.height as f32 - 58.0,
            right: self.width as f32 - 24.0,
            bottom: self.height as f32 - 18.0,
        };
        self.d2d.fill_rounded_rect(save_rect, 10.0, accent);
        self.d2d.draw_text(
            "Save",
            save_rect,
            D2D1_COLOR_F {
                r: 0.05,
                g: 0.07,
                b: 0.1,
                a: 1.0,
            },
            14.0,
            true,
            true,
        );

        let _ = self.d2d.end_draw();
    }
}

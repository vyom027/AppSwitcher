use crate::animations::TransitionKind;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::layouts::coverflow::CoverflowLayout;
use crate::layouts::dock::DockLayout;
use crate::layouts::fan::FanLayout;
use crate::layouts::grid::GridLayout;
use crate::layouts::hero::HeroLayout;
use crate::layouts::row::RowLayout;
use crate::layouts::wallet::WalletLayout;
use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::render::spring::{Spring, SpringConfig};
use crate::window::capture::{capture_window_image, screen_size, ImageBuffer};
use crate::window::enumerator::WindowInfo;
use crate::window::foreground::{bring_to_front, nudge_cursor};
use crossbeam_channel::Receiver;
use std::mem::size_of;
use std::thread;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LEFT, VK_MENU, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, PeekMessageW, RegisterClassExW,
    SetWindowPos, ShowWindow, TranslateMessage, HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOW, WM_QUIT, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    AltTab,
    Touchpad,
}

pub struct OverlayWindow {
    pub hwnd: HWND,
    pub width: u32,
    pub height: u32,
    pub d2d: D2DContext,
}

unsafe impl Send for OverlayWindow {}
unsafe impl Sync for OverlayWindow {}

unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: u32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, w_param, l_param)
}

impl OverlayWindow {
    pub fn new() -> Result<Self, windows::core::Error> {
        let (sw, sh) = screen_size();
        let width = sw as u32;
        let height = sh as u32;

        unsafe {
            let h_instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherOverlayClass");

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(overlay_wnd_proc);
            wc.hInstance = h_instance.into();
            wc.lpszClassName = class_name;

            let _ = RegisterClassExW(&wc);

            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("AppSwitcherOverlay"),
                style,
                0,
                0,
                sw,
                sh,
                HWND(std::ptr::null_mut()),
                None,
                h_instance,
                None,
            )?;

            // 1. Extend DWM frame into client area for native translucent background
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            // 2. Enable Windows 11 Native Acrylic Frosted Glass
            let backdrop_type = DWMSBT_TRANSIENTWINDOW.0 as u32;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &backdrop_type as *const _ as *const _,
                size_of::<u32>() as u32,
            );

            let d2d = D2DContext::new(hwnd, width, height)?;

            Ok(Self {
                hwnd,
                width,
                height,
                d2d,
            })
        }
    }

    pub fn show(&self) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOW);
        }
    }

    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Run full-screen switch transition between current and next app at 120Hz
    pub fn run_switch_transition(
        &self,
        direction: i32,
        curr_img: &ImageBuffer,
        next_img: &ImageBuffer,
        next_hwnd: HWND,
        anim_kind: TransitionKind,
        duration_secs: f32,
    ) {
        self.show();

        let sw = self.width as f32;
        let sh = self.height as f32;

        let curr_bmp = match self
            .d2d
            .create_bitmap_from_rgba(curr_img.width, curr_img.height, &curr_img.pixels)
        {
            Ok(b) => b,
            Err(_) => {
                bring_to_front(next_hwnd);
                self.hide();
                return;
            }
        };

        let next_bmp = match self
            .d2d
            .create_bitmap_from_rgba(next_img.width, next_img.height, &next_img.pixels)
        {
            Ok(b) => b,
            Err(_) => {
                bring_to_front(next_hwnd);
                self.hide();
                return;
            }
        };

        let start_time = Instant::now();
        let target_duration = Duration::from_secs_f32(duration_secs.max(0.08));

        // 120Hz/144Hz frame loop
        loop {
            let elapsed = start_time.elapsed();
            let t = (elapsed.as_secs_f32() / target_duration.as_secs_f32()).min(1.0);

            // Out-Quart easing curve for high-end snappy feel
            let eased_t = 1.0 - (1.0 - t).powi(4);

            self.d2d.begin_draw();
            // Clear with translucent frosted glass tint
            self.d2d.clear(0.04, 0.05, 0.08, 0.45);

            anim_kind.render(
                &self.d2d,
                &curr_bmp,
                &next_bmp,
                eased_t,
                direction,
                sw,
                sh,
            );

            let _ = self.d2d.end_draw();

            if t >= 1.0 {
                break;
            }

            thread::sleep(Duration::from_millis(7)); // ~140 fps timing budget
        }

        bring_to_front(next_hwnd);
        nudge_cursor();
        self.hide();
    }

    /// Interactive App Picker Modal with 7 interchangeable layouts & spring physics
    pub fn run_picker(
        &self,
        windows: &[WindowInfo],
        initial_idx: usize,
        layout_name: &str,
        accent: D2D1_COLOR_F,
        mode: PickerMode,
        use_ctrl: bool,
        event_rx: &Receiver<AppEvent>,
    ) -> Option<usize> {
        let n = windows.len();
        if n == 0 {
            return None;
        }

        self.show();

        let sw = self.width as f32;
        let sh = self.height as f32;

        let mut layout: Box<dyn PickerLayout> = match layout_name.to_lowercase().as_str() {
            "grid" => Box::new(GridLayout::default()),
            "coverflow" => Box::new(CoverflowLayout::default()),
            "row" => Box::new(RowLayout::default()),
            "hero" => Box::new(HeroLayout::default()),
            "wallet" => Box::new(WalletLayout::default()),
            "fan" => Box::new(FanLayout::default()),
            _ => Box::new(DockLayout::default()),
        };

        // Pre-create GPU bitmaps ONCE before the 120Hz loop
        // (Eliminates all per-frame GPU allocations, ensuring rock-solid 144Hz)
        let bitmaps: Vec<Option<ID2D1Bitmap>> = windows
            .iter()
            .map(|w| {
                capture_window_image(w.hwnd).and_then(|img| {
                    self.d2d
                        .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                        .ok()
                })
            })
            .collect();

        let initial = initial_idx.min(n - 1);
        let mut spring = Spring::new(initial as f64, SpringConfig::SNAPPY);
        let mut selected = initial;
        let mut fnav_armed = false;
        let mut commit_selection = false;
        let mut last_frame = Instant::now();
        let mut prev_left = false;
        let mut prev_right = false;

        loop {
            let dt = last_frame.elapsed().as_secs_f64();
            last_frame = Instant::now();

            // 1. Process Windows Messages & Channel Events
            let mut msg = MSG::default();
            let mut should_exit = false;

            unsafe {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        self.hide();
                        return None;
                    }
                    if msg.message == WM_APP_EVENT {
                        while let Ok(event) = event_rx.try_recv() {
                            match event {
                                AppEvent::AltTab { direction } => {
                                    selected = (selected as i32 + direction).rem_euclid(n as i32) as usize;
                                    spring.set_target(selected as f64);
                                }
                                AppEvent::AltTabRelease => {
                                    if mode == PickerMode::AltTab {
                                        commit_selection = true;
                                        should_exit = true;
                                    }
                                }
                                AppEvent::Swipe(dir) => {
                                    selected = (selected as i32 + dir).rem_euclid(n as i32) as usize;
                                    spring.set_target(selected as f64);
                                }
                                AppEvent::Quit => {
                                    self.hide();
                                    return None;
                                }
                                _ => {}
                            }
                        }
                    }
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if should_exit {
                break;
            }

            // 2. Hardware state checks per mode
            if mode == PickerMode::AltTab {
                let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
                let is_mod_down = unsafe { (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0 };
                if !is_mod_down {
                    commit_selection = true;
                    break;
                }
            } else if mode == PickerMode::Touchpad {
                let live = crate::input::get_live_state();
                if live.count >= 2 {
                    fnav_armed = true;
                    let target_f = (live.avg_x / 210.0).clamp(0.0, (n - 1) as f64);
                    spring.set_target(target_f);
                } else if live.count == 0 && fnav_armed {
                    commit_selection = true;
                    break;
                }
            }

            // Escape key -> cancel without switching
            unsafe {
                if (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = false;
                    break;
                }
                if (GetAsyncKeyState(VK_RETURN.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = true;
                    break;
                }

                // Left / Right arrow navigation
                let cur_left = (GetAsyncKeyState(VK_LEFT.0 as i32) as u16 & 0x8000) != 0;
                let cur_right = (GetAsyncKeyState(VK_RIGHT.0 as i32) as u16 & 0x8000) != 0;

                if cur_left && !prev_left {
                    selected = (selected + n - 1) % n;
                    spring.set_target(selected as f64);
                }
                if cur_right && !prev_right {
                    selected = (selected + 1) % n;
                    spring.set_target(selected as f64);
                }
                prev_left = cur_left;
                prev_right = cur_right;

                // Mouse Click check
                if (GetAsyncKeyState(1) as u16 & 0x8000) != 0 {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    if let Some(hit) = layout.hit_test(pt.x as f32, pt.y as f32, n, sw, sh) {
                        selected = hit;
                        commit_selection = true;
                        break;
                    }
                }
            }

            // 3. Update physics & layout
            spring.update(dt);
            if mode == PickerMode::Touchpad && fnav_armed {
                selected = (spring.value.round() as usize).clamp(0, n - 1);
            }
            layout.update(spring.value as f32, selected);

            // 4. Render 120Hz frame
            self.d2d.begin_draw();
            // Clear with translucent frosted glass tint
            // (Windows 11 hardware-accelerated Acrylic blur shows through)
            self.d2d.clear(0.04, 0.05, 0.08, 0.45);

            // Active layout
            layout.render(&self.d2d, windows, &bitmaps, accent, sw, sh);

            let _ = self.d2d.end_draw();

            thread::sleep(Duration::from_millis(7)); // 140Hz timing sync
        }

        self.hide();
        if commit_selection {
            Some(selected)
        } else {
            None
        }
    }
}

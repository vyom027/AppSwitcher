use crate::animations::TransitionKind;
use crate::event::{AppEvent, WM_APP_EVENT};
use crate::layouts::dock::DockLayout;
use crate::layouts::expose_cloud::ExposeCloudLayout;
use crate::layouts::hero::HeroLayout;
use crate::layouts::row::RowLayout;
use crate::layouts::spotlight::SpotlightLayout;
use crate::layouts::{draw_liquid_glass, PickerLayout};
use crate::render::d2d::D2DContext;
use crate::render::spring::{Spring, SpringConfig};
use crate::window::capture::{capture_windows_parallel, get_backdrop, work_area, ImageBuffer};
use crate::window::enumerator::WindowInfo;
use crate::window::foreground::{bring_to_front, nudge_cursor};
use crossbeam_channel::Receiver;
use std::mem::size_of;
use std::thread;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWINDOWATTRIBUTE,
};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LEFT, VK_MBUTTON, VK_MENU, VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, PeekMessageW, PostMessageW,
    RegisterClassExW, SetWindowPos, TranslateMessage, HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, WM_CLOSE, WM_QUIT, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    AltTab,
    Touchpad,
    Preview,
}

pub struct OverlayWindow {
    pub hwnd: HWND,
    pub width: std::sync::atomic::AtomicU32,
    pub height: std::sync::atomic::AtomicU32,
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
    match msg {
        windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => LRESULT(1), // Prevent GDI white background flash
        windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
            let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
            let _ = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
            let _ = windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, w_param, l_param),
    }
}

struct OverlayGuard<'a>(&'a OverlayWindow);
impl<'a> Drop for OverlayGuard<'a> {
    fn drop(&mut self) {
        self.0.hide();
    }
}

struct SearchModeGuard;
impl Drop for SearchModeGuard {
    fn drop(&mut self) {
        crate::input::keyboard_hook::set_search_mode(false);
    }
}

fn move_within(candidates: &[usize], selected: usize, direction: i32) -> (usize, usize) {
    if candidates.is_empty() {
        return (selected, 0);
    }
    let current = candidates
        .iter()
        .position(|&idx| idx == selected)
        .unwrap_or(0);
    let next = (current as i32 + direction).rem_euclid(candidates.len() as i32) as usize;
    (candidates[next], next)
}

impl OverlayWindow {
    pub fn new() -> Result<Self, windows::core::Error> {
        let (wx, wy, ww, wh) = work_area();
        let width = ww.max(100) as u32;
        let height = wh.max(100) as u32;

        unsafe {
            let h_instance = GetModuleHandleW(None)?;
            let class_name = w!("AppSwitcherOverlayClass");

            let mut wc = WNDCLASSEXW::default();
            wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(overlay_wnd_proc);
            wc.hInstance = h_instance.into();
            wc.lpszClassName = class_name;
            wc.hbrBackground = HBRUSH(GetStockObject(BLACK_BRUSH).0);

            let _ = RegisterClassExW(&wc);

            let ex_style = WS_EX_TOPMOST | WS_EX_TOOLWINDOW;
            let style = WS_POPUP;

            let hwnd = CreateWindowExW(
                ex_style,
                class_name,
                w!("AppSwitcherOverlay"),
                style,
                wx,
                wy,
                ww,
                wh,
                HWND(std::ptr::null_mut()),
                None,
                h_instance,
                None,
            )?;

            // 1. Dark mode DWM frame and immersion attributes (Eliminates white flashes)
            let dark_mode = windows::Win32::Foundation::BOOL(1);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20),
                &dark_mode as *const _ as *const _,
                size_of::<windows::Win32::Foundation::BOOL>() as u32,
            );

            // 2. Extend DWM frame into client area
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);

            let d2d = D2DContext::new(hwnd, width, height)?;

            Ok(Self {
                hwnd,
                width: std::sync::atomic::AtomicU32::new(width),
                height: std::sync::atomic::AtomicU32::new(height),
                d2d,
            })
        }
    }

    pub fn sync_work_area(&self) -> (f32, f32) {
        let (wx, wy, ww, wh) = work_area();
        let ww_u = ww.max(100) as u32;
        let wh_u = wh.max(100) as u32;

        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, wx, wy, ww, wh, SWP_NOACTIVATE);
        }

        let old_w = self.width.swap(ww_u, std::sync::atomic::Ordering::SeqCst);
        let old_h = self.height.swap(wh_u, std::sync::atomic::Ordering::SeqCst);

        if old_w != ww_u || old_h != wh_u {
            self.d2d.resize(ww_u, wh_u);
        }

        (ww as f32, wh as f32)
    }

    pub fn show(&self, clickable: bool) {
        crate::window::capture::OVERLAY_VISIBLE.store(true, std::sync::atomic::Ordering::SeqCst);
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                SetWindowLongW, ShowWindow, GWL_EXSTYLE, SW_SHOWNA, WS_EX_TOOLWINDOW,
                WS_EX_TOPMOST, WS_EX_TRANSPARENT,
            };
            let mut ex = (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0) as i32;
            if !clickable {
                ex |= WS_EX_TRANSPARENT.0 as i32;
            }
            let _ = SetWindowLongW(self.hwnd, GWL_EXSTYLE, ex);
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW | SWP_NOACTIVATE,
            );
            let _ = ShowWindow(self.hwnd, SW_SHOWNA);
        }
    }

    pub fn hide(&self) {
        crate::window::capture::OVERLAY_VISIBLE.store(false, std::sync::atomic::Ordering::SeqCst);
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Run full-screen switch transition between current and next app at 120Hz/144Hz
    pub fn run_switch_transition(
        &self,
        direction: i32,
        curr_img: &ImageBuffer,
        next_img: &ImageBuffer,
        next_hwnd: HWND,
        anim_kind: TransitionKind,
        duration_secs: f32,
        commit_focus: bool,
    ) {
        let _guard = OverlayGuard(self);
        let (sw, sh) = self.sync_work_area();

        // 1. Capture backdrop and create bitmaps BEFORE revealing the window!
        let bg = get_backdrop();
        let bg_bmp = self
            .d2d
            .create_bitmap_from_rgba(bg.width, bg.height, &bg.pixels)
            .ok();

        let curr_bmp = match self.d2d.create_bitmap_from_rgba(
            curr_img.width,
            curr_img.height,
            &curr_img.pixels,
        ) {
            Ok(b) => b,
            Err(_) => {
                if commit_focus {
                    bring_to_front(next_hwnd);
                }
                self.hide();
                return;
            }
        };

        let next_bmp = match self.d2d.create_bitmap_from_rgba(
            next_img.width,
            next_img.height,
            &next_img.pixels,
        ) {
            Ok(b) => b,
            Err(_) => {
                if commit_focus {
                    bring_to_front(next_hwnd);
                }
                self.hide();
                return;
            }
        };

        // 2. Pre-render initial frame 0 to D2D surface before window is shown (Zero white screen!)
        self.d2d.begin_draw();
        if let Some(ref bg) = bg_bmp {
            self.d2d.draw_bitmap(
                bg,
                D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                },
                1.0,
            );
        } else {
            self.d2d.clear(0.04, 0.05, 0.08, 1.0);
        }
        anim_kind.render(&self.d2d, &curr_bmp, &next_bmp, 0.0, direction, sw, sh);
        let _ = self.d2d.end_draw();

        // 3. Now reveal window cleanly (click-through during transitions)
        self.show(false);

        let start_time = Instant::now();
        let target_duration = Duration::from_secs_f32(duration_secs.max(0.12));
        let mut cancelled = false;

        // 120Hz/144Hz frame loop
        loop {
            if unsafe { (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 } {
                cancelled = true;
                break;
            }
            let elapsed = start_time.elapsed();
            let t = (elapsed.as_secs_f32() / target_duration.as_secs_f32()).min(1.0);

            // Drain queued notifications while animating. Leaving WM_APP_EVENT at
            // the head of the queue makes this loop spin without pumping paint,
            // close, or input messages; the main loop is woken again after the
            // transition completes.
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            // Out-Cubic easing curve for organic motion
            let eased_t = 1.0 - (1.0 - t).powi(3);

            self.d2d.begin_draw();

            // 1. Draw photorealistic blurred frosted desktop backdrop
            if let Some(ref bg) = bg_bmp {
                self.d2d.draw_bitmap(
                    bg,
                    D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: sw,
                        bottom: sh,
                    },
                    1.0,
                );
            } else {
                self.d2d.clear(0.06, 0.08, 0.12, 1.0);
            }

            // 2. Render animation
            anim_kind.render(&self.d2d, &curr_bmp, &next_bmp, eased_t, direction, sw, sh);

            let _ = self.d2d.end_draw();

            if t >= 1.0 {
                break;
            }

            thread::sleep(Duration::from_millis(7)); // ~140 fps timing budget
        }

        if !cancelled && commit_focus {
            bring_to_front(next_hwnd);
        }
        // Allow target window to composite underneath overlay before hiding (eliminates 10ms white screen)
        thread::sleep(Duration::from_millis(20));
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        self.hide();
        // Some input can arrive after the last animation-frame drain. Ensure
        // the outer event loop wakes even if its WM_APP_EVENT was just drained.
        unsafe {
            let _ = PostMessageW(self.hwnd, WM_APP_EVENT, WPARAM(0), LPARAM(0));
        }
        nudge_cursor();

        // Trigger background prewarm only after a real switch.
        if commit_focus {
            let wins = crate::window::get_open_windows(&[], false);
            let mru = crate::window::sort_by_mru(wins);
            if mru.len() > 1 {
                let mut targets = vec![mru[0].hwnd, mru[1].hwnd];
                if mru.len() > 2 {
                    targets.push(mru[mru.len() - 1].hwnd);
                }
                crate::window::capture::prewarm_windows(&targets);
            }
        }
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
        sensitivity: f32,
        event_rx: &Receiver<AppEvent>,
    ) -> Option<(usize, Option<ImageBuffer>, Option<ImageBuffer>)> {
        let _guard = OverlayGuard(self);
        let n = windows.len();
        if n == 0 {
            return None;
        }

        let (sw, sh) = self.sync_work_area();

        let mut layout: Box<dyn PickerLayout> = match layout_name.to_lowercase().as_str() {
            "row" => Box::new(RowLayout::default()),
            "hero" => Box::new(HeroLayout::default()),
            "expose_cloud" => Box::new(ExposeCloudLayout::default()),
            "spotlight" => Box::new(SpotlightLayout::default()),
            _ => Box::new(DockLayout::default()),
        };

        // 1. Load frosted desktop backdrop
        let bg = get_backdrop();
        let bg_bmp = self
            .d2d
            .create_bitmap_from_rgba(bg.width, bg.height, &bg.pixels)
            .ok();

        // 2. Concurrently capture thumbnails in parallel (Eliminates all UI lag!)
        let hwnds: Vec<HWND> = windows.iter().map(|w| w.hwnd).collect();
        let mut raw_images = capture_windows_parallel(&hwnds);

        let mut bitmaps: Vec<Option<ID2D1Bitmap>> = raw_images
            .iter()
            .map(|opt_img| {
                opt_img.as_ref().and_then(|img| {
                    self.d2d
                        .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                        .ok()
                })
            })
            .collect();

        // Single, clean live thumbnail background refresh worker (updates every 2.5s matching Python)
        let (refresh_tx, refresh_rx) = crossbeam_channel::unbounded::<(usize, ImageBuffer)>();
        let picker_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let bg_active = std::sync::Arc::clone(&picker_active);
        let bg_hwnds: Vec<usize> = hwnds.iter().map(|h| h.0 as usize).collect();

        thread::spawn(move || {
            while bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                // Sleep in 100ms intervals so thread exits promptly on picker close
                for _ in 0..25 {
                    thread::sleep(Duration::from_millis(100));
                    if !bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                }
                for (idx, &raw_h) in bg_hwnds.iter().enumerate() {
                    if !bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let h = HWND(raw_h as *mut _);
                    if let Some(img) = crate::window::capture::capture_window_image_uncached(h) {
                        let mut has_color = false;
                        for px in img.pixels.chunks_exact(16) {
                            if px[0] > 15 || px[1] > 15 || px[2] > 15 {
                                has_color = true;
                                break;
                            }
                        }
                        if has_color {
                            let _ = refresh_tx.send((idx, img));
                        }
                    }
                }
            }
        });

        // 3. Concurrently retrieve window icons
        let icons: Vec<Option<ID2D1Bitmap>> = windows
            .iter()
            .map(|w| {
                crate::window::get_window_icon(w.hwnd).and_then(|img| {
                    self.d2d
                        .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                        .ok()
                })
            })
            .collect();

        let initial = initial_idx.min(n - 1);
        let mut spring = Spring::new(initial as f64, SpringConfig::SNAPPY);
        let mut selected = initial;

        // Relative touchpad navigation state
        let mut fnav_base_x: Option<f64> = None;
        let mut fnav_base_focus: f64 = initial as f64;
        let mut fnav_armed = false;

        let mut commit_selection = false;
        let mut last_frame = Instant::now();
        let mut prev_left = false;
        let mut prev_right = false;

        // Top Pill Hint Text
        let hint_text = match mode {
            PickerMode::AltTab => {
                let mod_name = if use_ctrl { "Ctrl" } else { "Alt" };
                format!(
                    "hold {} · Tab to cycle · release {} to switch · Middle-click close · Esc cancel",
                    mod_name, mod_name
                )
            }
            PickerMode::Touchpad => {
                "glide fingers to pick · lift to switch · Middle-click close · Esc cancel"
                    .to_string()
            }
            PickerMode::Preview => {
                "Preview · click a window or press Enter to switch · Esc to close".to_string()
            }
        };

        // Quick Alt+Tab tap check before window reveal
        if mode == PickerMode::AltTab {
            let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
            let is_mod_down = unsafe { (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0 };
            if !is_mod_down {
                let curr_img = raw_images.get(0).cloned().flatten();
                let next_img = raw_images.get(initial).cloned().flatten();
                return Some((initial, curr_img, next_img));
            }
        }

        let is_spotlight = layout_name.eq_ignore_ascii_case("spotlight");
        let _search_mode = if is_spotlight {
            crate::input::keyboard_hook::set_search_mode(true);
            Some(SearchModeGuard)
        } else {
            None
        };
        let mut search_query = String::new();

        self.show(true);
        let show_start = Instant::now();
        let fade_dur = 0.12f32;

        loop {
            let dt = last_frame.elapsed().as_secs_f64();
            last_frame = Instant::now();

            // 1. Process Windows Messages & Channel Events
            let mut msg = MSG::default();
            let mut should_exit = false;
            let mut candidates = layout.visible_indices(windows);

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
                                    let (idx, position) =
                                        move_within(&candidates, selected, direction);
                                    selected = idx;
                                    spring.set_target(position as f64);
                                }
                                AppEvent::AltTabRelease => {
                                    if mode == PickerMode::AltTab {
                                        commit_selection = !candidates.is_empty();
                                        should_exit = true;
                                    }
                                }
                                AppEvent::Swipe(dir) => {
                                    let (idx, position) = move_within(&candidates, selected, dir);
                                    selected = idx;
                                    spring.set_target(position as f64);
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

            if is_spotlight {
                let mut changed = false;
                for input in crate::input::keyboard_hook::take_search_input() {
                    match input {
                        crate::input::keyboard_hook::SearchInput::Character(ch)
                            if search_query.chars().count() < 64 =>
                        {
                            search_query.push(ch);
                            changed = true;
                        }
                        crate::input::keyboard_hook::SearchInput::Backspace => {
                            if search_query.pop().is_some() {
                                changed = true;
                            }
                        }
                        crate::input::keyboard_hook::SearchInput::Clear => {
                            if !search_query.is_empty() {
                                search_query.clear();
                                changed = true;
                            }
                        }
                        _ => {}
                    }
                }
                if changed {
                    layout.set_query(&search_query);
                    candidates = layout.visible_indices(windows);
                    let position = candidates
                        .iter()
                        .position(|&idx| idx == selected)
                        .unwrap_or(0);
                    if let Some(&idx) = candidates.get(position) {
                        selected = idx;
                    }
                    spring.set_target(position as f64);
                }
            }

            // Receive any background refreshed live frames (strictly guard against blank/black frames)
            while let Ok((idx, img)) = refresh_rx.try_recv() {
                let mut valid = false;
                for px in img.pixels.chunks_exact(16) {
                    if px[0] > 15 || px[1] > 15 || px[2] > 15 {
                        valid = true;
                        break;
                    }
                }
                if valid {
                    if let Ok(bmp) =
                        self.d2d
                            .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                    {
                        if idx < bitmaps.len() {
                            bitmaps[idx] = Some(bmp);
                        }
                        if idx < raw_images.len() {
                            raw_images[idx] = Some(img);
                        }
                    }
                }
            }

            // 2. Hardware state checks per mode
            if mode == PickerMode::AltTab {
                let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
                let is_mod_down =
                    unsafe { (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0 };
                if !is_mod_down {
                    commit_selection = true;
                    break;
                }
            } else if mode == PickerMode::Touchpad {
                let live = crate::input::get_live_state();
                if live.count >= 2 && !candidates.is_empty() {
                    if fnav_base_x.is_none() {
                        fnav_base_x = Some(live.avg_x);
                        fnav_base_focus = spring.value;
                        fnav_armed = true;
                    }
                    let dx = live.avg_x - fnav_base_x.unwrap();
                    let step = sensitivity.max(80.0) as f64;
                    let target_f =
                        (fnav_base_focus + dx / step).clamp(0.0, (candidates.len() - 1) as f64);
                    spring.set_target(target_f);
                } else if live.count == 0 {
                    // Fingers lifted! If armed or if fingers were already lifted after brief grace period (120ms):
                    if fnav_armed || show_start.elapsed() > Duration::from_millis(120) {
                        commit_selection = !candidates.is_empty();
                        break;
                    }
                }
            }

            // Hard watchdog timeout: Never allow picker to remain stuck for more than 8 seconds
            if show_start.elapsed() > Duration::from_secs(8) {
                commit_selection = false;
                break;
            }

            // Keyboard & Mouse navigation checks
            unsafe {
                if (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = false;
                    break;
                }
                if (GetAsyncKeyState(VK_RETURN.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = !candidates.is_empty();
                    break;
                }

                // Left / Right arrow navigation
                let cur_left = (GetAsyncKeyState(VK_LEFT.0 as i32) as u16 & 0x8000) != 0;
                let cur_right = (GetAsyncKeyState(VK_RIGHT.0 as i32) as u16 & 0x8000) != 0;

                if cur_left && !prev_left {
                    let (idx, position) = move_within(&candidates, selected, -1);
                    selected = idx;
                    spring.set_target(position as f64);
                }
                if cur_right && !prev_right {
                    let (idx, position) = move_within(&candidates, selected, 1);
                    selected = idx;
                    spring.set_target(position as f64);
                }
                prev_left = cur_left;
                prev_right = cur_right;

                // Middle-click to close window
                if (GetAsyncKeyState(VK_MBUTTON.0 as i32) as u16 & 0x8000) != 0 {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    if let Some(hit) = layout.hit_test(pt.x as f32, pt.y as f32, windows, sw, sh) {
                        let _ = PostMessageW(windows[hit].hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                        commit_selection = false;
                        break;
                    }
                }

                // Left-click to select window, or click outside to dismiss
                if (GetAsyncKeyState(1) as u16 & 0x8000) != 0 {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    if let Some(hit) = layout.hit_test(pt.x as f32, pt.y as f32, windows, sw, sh) {
                        selected = hit;
                        commit_selection = true;
                        break;
                    } else if show_start.elapsed() > Duration::from_millis(220) {
                        // Clicked backdrop / outside cards: dismiss cleanly
                        commit_selection = false;
                        break;
                    }
                }
            }

            // 3. Update physics & layout
            spring.update(dt);
            if ((mode == PickerMode::Touchpad && fnav_armed) || mode == PickerMode::AltTab)
                && !candidates.is_empty()
            {
                let position = (spring.value.round() as usize).min(candidates.len() - 1);
                selected = candidates[position];
            }

            layout.update(spring.value as f32, selected);

            // 4. Render 120Hz/144Hz frame with smooth fade-in
            let alpha = (show_start.elapsed().as_secs_f32() / fade_dur).min(1.0);
            let eased_alpha = 1.0 - (1.0 - alpha).powi(3);

            self.d2d.begin_draw();

            // Draw blurred desktop frosted glass backdrop
            if let Some(ref bg) = bg_bmp {
                self.d2d.draw_bitmap(
                    bg,
                    D2D_RECT_F {
                        left: 0.0,
                        top: 0.0,
                        right: sw,
                        bottom: sh,
                    },
                    eased_alpha,
                );
            } else {
                self.d2d.clear(0.04, 0.05, 0.08, eased_alpha);
            }

            // Top Pill Badge (Refined Liquid Glass Capsule)
            let pill_w = (hint_text.len() as f32 * 6.8 + 32.0).clamp(260.0, 520.0);
            let pill_rect = D2D_RECT_F {
                left: (sw - pill_w) * 0.5,
                top: 14.0,
                right: (sw + pill_w) * 0.5,
                bottom: 40.0,
            };
            draw_liquid_glass(&self.d2d, pill_rect, 13.0, accent, false);
            self.d2d.draw_text(
                &hint_text,
                pill_rect,
                D2D1_COLOR_F {
                    r: 0.88,
                    g: 0.90,
                    b: 0.95,
                    a: 0.90 * eased_alpha,
                },
                11.0,
                false,
                true,
            );

            // Active layout
            layout.render(&self.d2d, windows, &bitmaps, &icons, accent, sw, sh);

            let _ = self.d2d.end_draw();

            thread::sleep(Duration::from_millis(7)); // 140Hz timing sync
        }

        picker_active.store(false, std::sync::atomic::Ordering::SeqCst);
        self.hide();
        if commit_selection {
            let curr_img = raw_images.get(0).cloned().flatten();
            let next_img = raw_images.get(selected).cloned().flatten();
            Some((selected, curr_img, next_img))
        } else {
            None
        }
    }
}

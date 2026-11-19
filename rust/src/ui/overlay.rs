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
use crate::window::capture::{
    capture_windows_parallel, get_backdrop, window_work_area, work_area, ImageBuffer,
};
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
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetForegroundWindow, IsWindow,
    PeekMessageW, PostMessageW,
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
    /// Screen position of the overlay's top-left corner; layouts work in
    /// client coordinates, so cursor positions are offset by this.
    origin_x: std::sync::atomic::AtomicI32,
    origin_y: std::sync::atomic::AtomicI32,
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
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEWHEEL => {
            // High word of wParam is the signed wheel delta (120 per notch).
            let delta = ((w_param.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
            WHEEL_ACCUM.fetch_add(delta, std::sync::atomic::Ordering::SeqCst);
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

/// Close the picker after this long with no user activity.
const PICKER_IDLE_TIMEOUT: Duration = Duration::from_secs(8);

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
            // Without a class cursor Windows keeps showing whatever cursor was
            // last set (often the busy cursor) while over the picker.
            wc.hCursor = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(
                None,
                windows::Win32::UI::WindowsAndMessaging::IDC_ARROW,
            )
            .unwrap_or_default();

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
                origin_x: std::sync::atomic::AtomicI32::new(wx),
                origin_y: std::sync::atomic::AtomicI32::new(wy),
                d2d,
            })
        }
    }

    /// Move/resize the overlay to cover `area` (x, y, w, h in screen pixels).
    pub fn sync_to_area(&self, area: (i32, i32, i32, i32)) -> (f32, f32) {
        let (wx, wy, ww, wh) = area;
        self.origin_x.store(wx, std::sync::atomic::Ordering::SeqCst);
        self.origin_y.store(wy, std::sync::atomic::Ordering::SeqCst);
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

    /// Cursor position in overlay client coordinates (what layouts use).
    fn cursor_in_client(&self) -> (f32, f32) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        (
            (pt.x - self.origin_x.load(std::sync::atomic::Ordering::Relaxed)) as f32,
            (pt.y - self.origin_y.load(std::sync::atomic::Ordering::Relaxed)) as f32,
        )
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
        // Play the transition on the monitor of the window being left.
        let area = unsafe {
            let fg = GetForegroundWindow();
            if fg.0.is_null()
                || crate::window::foreground::is_own_window(fg)
                || !crate::window::enumerator::is_alt_tab_window(fg)
            {
                // Desktop/taskbar focused: use the monitor under the cursor,
                // matching the screen snapshot used as the "from" frame.
                work_area()
            } else {
                window_work_area(fg)
            }
        };
        let (sw, sh) = self.sync_to_area(area);

        // 1. Capture backdrop and create bitmaps BEFORE revealing the window!
        let bg = get_backdrop(area);
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

        // A window that isn't maximized animates at its real size and position
        // over the current screen, instead of as a full-screen frame. `src` is
        // the window inside its capture; `dest` is where it sits on screen.
        let windowed = next_img.content.and_then(|(x, y, w, h)| {
            let image_area = next_img.width as f32 * next_img.height as f32;
            if w <= 0 || h <= 0 || (w as f32 * h as f32) >= image_area * 0.9 {
                return None;
            }
            let kx = sw / next_img.width as f32;
            let ky = sh / next_img.height as f32;
            let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
            Some((
                D2D_RECT_F {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                D2D_RECT_F {
                    left: x * kx,
                    top: y * ky,
                    right: (x + w) * kx,
                    bottom: (y + h) * ky,
                },
            ))
        });
        let draw_transition = |t: f32| match windowed {
            Some((src, dest)) => crate::animations::windowed::render(
                &anim_kind, &self.d2d, &curr_bmp, &next_bmp, src, dest, t, direction, sw, sh,
            ),
            None => anim_kind.render(&self.d2d, &curr_bmp, &next_bmp, t, direction, sw, sh),
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
        draw_transition(0.0);
        let _ = self.d2d.end_draw();

        // 3. Now reveal window cleanly (click-through during transitions)
        self.show(false);

        let start_time = Instant::now();
        let target_duration =
            Duration::from_secs_f32(duration_secs.max(0.12) * anim_kind.duration_scale());
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
            draw_transition(eased_t);

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

    /// Interactive App Picker Modal with interchangeable layouts & spring physics.
    ///
    /// Returns the selected index into `windows` (the caller's original list,
    /// even if cards were closed from the picker) plus the captured frames of
    /// the current and selected windows for the follow-up transition.
    pub fn run_picker(
        &self,
        windows: &[WindowInfo],
        initial_idx: usize,
        layout_name: &str,
        accent: D2D1_COLOR_F,
        mode: PickerMode,
        use_ctrl: bool,
        sensitivity: f32,
        dock_mag: f32,
        event_rx: &Receiver<AppEvent>,
    ) -> Option<(usize, Option<ImageBuffer>, Option<ImageBuffer>)> {
        let _guard = OverlayGuard(self);
        let original = windows;
        if original.is_empty() {
            return None;
        }
        // Owned copy: cards can be removed when their window is closed.
        let mut windows: Vec<WindowInfo> = original.to_vec();

        let area = work_area();
        let (sw, sh) = self.sync_to_area(area);
        let reduce_motion = !crate::animations::motion_allowed();

        let mut layout: Box<dyn PickerLayout> = match layout_name.to_lowercase().as_str() {
            "row" => Box::new(RowLayout::default()),
            "hero" => Box::new(HeroLayout::default()),
            "expose_cloud" => Box::new(ExposeCloudLayout::default()),
            "spotlight" => Box::new(SpotlightLayout::default()),
            "orbit" => Box::new(crate::layouts::orbit::OrbitLayout::default()),
            "honeycomb" => Box::new(crate::layouts::honeycomb::HoneycombLayout::default()),
            _ => {
                let mut dock = DockLayout::default();
                // Settings slider range is 1.2-2.6; guard against hand-edited files.
                dock.mag = dock_mag.clamp(1.0, 3.0);
                Box::new(dock)
            }
        };

        // 1. Load frosted desktop backdrop
        let bg = get_backdrop(area);
        let bg_bmp = self
            .d2d
            .create_bitmap_from_rgba(bg.width, bg.height, &bg.pixels)
            .ok();

        // 2. Concurrently capture thumbnails in parallel
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

        // Live thumbnail refresh worker (every 2.5s). Frames are tagged with
        // the window handle, not an index, because cards can be removed.
        let (refresh_tx, refresh_rx) = crossbeam_channel::unbounded::<(usize, ImageBuffer)>();
        let picker_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let _stop_refresh = StopFlagGuard(std::sync::Arc::clone(&picker_active));
        let bg_active = std::sync::Arc::clone(&picker_active);
        let bg_hwnds: Vec<usize> = hwnds.iter().map(|h| h.0 as usize).collect();

        thread::spawn(move || {
            while bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                for _ in 0..25 {
                    thread::sleep(Duration::from_millis(100));
                    if !bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                }
                for &raw_h in &bg_hwnds {
                    if !bg_active.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    let h = HWND(raw_h as *mut _);
                    if !unsafe { IsWindow(h) }.as_bool() {
                        continue;
                    }
                    if let Some(img) = crate::window::capture::capture_window_image_uncached(h) {
                        if has_visible_content(&img) {
                            let _ = refresh_tx.send((raw_h, img));
                        }
                    }
                }
            }
        });

        // 3. Window icons
        let mut icons: Vec<Option<ID2D1Bitmap>> = windows
            .iter()
            .map(|w| {
                crate::window::get_window_icon(w.hwnd).and_then(|img| {
                    self.d2d
                        .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                        .ok()
                })
            })
            .collect();

        let initial = initial_idx.min(windows.len() - 1);
        // The spring tracks a position in the layout's navigation order, which
        // need not match window order (e.g. Honeycomb reads row by row).
        let initial_pos = layout
            .visible_indices(&windows)
            .iter()
            .position(|&i| i == initial)
            .unwrap_or(0);
        let mut spring = Spring::new(initial_pos as f64, SpringConfig::SNAPPY);
        let mut selected = initial;

        // Relative touchpad navigation state
        let mut fnav_base_x: Option<f64> = None;
        let mut fnav_base_focus: f64 = initial_pos as f64;
        let mut fnav_armed = false;

        let mut commit_selection = false;
        let mut last_frame = Instant::now();
        let mut prev_left = false;
        let mut prev_right = false;
        let mut prev_mbutton = false;

        let hint_text = match mode {
            PickerMode::AltTab => {
                let mod_name = if use_ctrl { "Ctrl" } else { "Alt" };
                format!(
                    "Tab · arrows · wheel to move · release {} to switch · Q close window · Esc cancel",
                    mod_name
                )
            }
            PickerMode::Touchpad => {
                "glide fingers to pick · lift to switch · Q / middle-click close · Esc cancel"
                    .to_string()
            }
            PickerMode::Preview => {
                "click or press Enter to switch · 1-9 jump · Q close window · Esc close"
                    .to_string()
            }
        };

        // Quick Alt+Tab tap check before window reveal
        if mode == PickerMode::AltTab {
            let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
            let is_mod_down = unsafe { (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0 };
            if !is_mod_down {
                let curr_img = raw_images.first().cloned().flatten();
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
        crate::input::keyboard_hook::set_picker_mode(true);
        let _picker_keys = PickerKeysGuard;
        let hook_keys = crate::input::keyboard_hook::is_installed();
        WHEEL_ACCUM.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut search_query = String::new();

        self.show(true);
        let show_start = Instant::now();
        let fade_dur = if reduce_motion { 0.0 } else { 0.12f32 };
        let mut last_activity = show_start;
        let mut last_target = spring.target;
        let mut last_query_len = 0usize;
        // Hover only reacts to real cursor movement, never to where the
        // cursor happened to be resting when the picker opened.
        let mut hover_cursor = self.cursor_in_client();

        loop {
            let dt = last_frame.elapsed().as_secs_f64();
            last_frame = Instant::now();

            let mut should_exit = false;
            let mut candidates = layout.visible_indices(&windows);
            // Navigation requested this frame: a relative step and/or an
            // absolute position within `candidates`.
            let mut step: i32 = 0;
            let mut jump_to: Option<usize> = None;
            let mut close_request: Option<usize> = None;

            // 1. Process Windows Messages & Channel Events
            unsafe {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_QUIT {
                        return None;
                    }
                    if msg.message == WM_APP_EVENT {
                        while let Ok(event) = event_rx.try_recv() {
                            match event {
                                AppEvent::AltTab { direction }
                                | AppEvent::AltTabSameApp { direction } => step += direction,
                                AppEvent::AltTabRelease => {
                                    if mode == PickerMode::AltTab {
                                        commit_selection = !candidates.is_empty();
                                        should_exit = true;
                                    }
                                }
                                AppEvent::Swipe(dir) => step += dir,
                                AppEvent::Quit => return None,
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
                    candidates = layout.visible_indices(&windows);
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

            // Background-refreshed live frames
            while let Ok((raw_h, img)) = refresh_rx.try_recv() {
                let Some(idx) = windows.iter().position(|w| w.hwnd.0 as usize == raw_h) else {
                    continue;
                };
                if has_visible_content(&img) {
                    if let Ok(bmp) =
                        self.d2d
                            .create_bitmap_from_rgba(img.width, img.height, &img.pixels)
                    {
                        bitmaps[idx] = Some(bmp);
                        raw_images[idx] = Some(img);
                    }
                }
            }

            // 2. Hardware state checks per mode
            if mode == PickerMode::AltTab {
                let mod_vk = if use_ctrl { VK_CONTROL } else { VK_MENU };
                let is_mod_down =
                    unsafe { (GetAsyncKeyState(mod_vk.0 as i32) as u16 & 0x8000) != 0 };
                if !is_mod_down {
                    commit_selection = !candidates.is_empty();
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
                    let step_px = sensitivity.max(80.0) as f64;
                    let target_f = (fnav_base_focus + dx / step_px)
                        .clamp(0.0, (candidates.len() - 1) as f64);
                    spring.set_target(target_f);
                } else if live.count == 0 {
                    // Fingers lifted after arming, or never on the pad after a short grace period.
                    if fnav_armed || show_start.elapsed() > Duration::from_millis(120) {
                        commit_selection = !candidates.is_empty();
                        break;
                    }
                }
            }

            // 3. Keyboard: keys queued by the low-level hook (swallowed so
            // they never reach the app underneath).
            for vk in crate::input::keyboard_hook::take_picker_keys() {
                match vk {
                    0x1B => {
                        should_exit = true;
                        commit_selection = false;
                    }
                    0x0D => {
                        should_exit = true;
                        commit_selection = !candidates.is_empty();
                    }
                    0x25 => step -= 1, // Left
                    0x27 => step += 1, // Right
                    0x26 | 0x28 => {
                        // Up / Down: to the row above or below in layouts
                        // with rows, otherwise the same as Left / Right.
                        let dir = if vk == 0x26 { -1 } else { 1 };
                        match layout
                            .step_vertical(selected, dir, &windows)
                            .and_then(|idx| candidates.iter().position(|&c| c == idx))
                        {
                            Some(pos) => jump_to = Some(pos),
                            None => step += dir,
                        }
                    }
                    0x24 => jump_to = Some(0), // Home
                    0x23 => jump_to = Some(candidates.len().saturating_sub(1)), // End
                    0x31..=0x39 => {
                        // 1-9: jump straight to that card and switch.
                        let pos = (vk - 0x31) as usize;
                        if let Some(&idx) = candidates.get(pos) {
                            selected = idx;
                            commit_selection = true;
                            should_exit = true;
                        }
                    }
                    0x51 | 0x57 | 0x2E => close_request = Some(selected), // Q, W, Delete
                    _ => {}
                }
            }
            if should_exit {
                break;
            }

            // Fallback key polling for when the hook is unavailable. With the
            // hook installed these keys are swallowed and never show up here.
            unsafe {
                if (GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = false;
                    break;
                }
                if (GetAsyncKeyState(VK_RETURN.0 as i32) as u16 & 0x8000) != 0 {
                    commit_selection = !candidates.is_empty();
                    break;
                }
                if !hook_keys {
                    let cur_left = (GetAsyncKeyState(VK_LEFT.0 as i32) as u16 & 0x8000) != 0;
                    let cur_right = (GetAsyncKeyState(VK_RIGHT.0 as i32) as u16 & 0x8000) != 0;
                    if cur_left && !prev_left {
                        step -= 1;
                    }
                    if cur_right && !prev_right {
                        step += 1;
                    }
                    prev_left = cur_left;
                    prev_right = cur_right;
                }
            }

            // 4. Mouse: wheel, hover, middle-click close, left-click select.
            let wheel_steps = take_wheel_steps();
            step += wheel_steps;

            let cursor = self.cursor_in_client();
            let cursor_moved = cursor != hover_cursor;
            if cursor_moved {
                hover_cursor = cursor;
                if let Some(hit) = layout.hit_test(cursor.0, cursor.1, &windows, sw, sh) {
                    if hit != selected {
                        if let Some(pos) = candidates.iter().position(|&i| i == hit) {
                            jump_to = Some(pos);
                        }
                    }
                }
            }

            unsafe {
                let mbutton = (GetAsyncKeyState(VK_MBUTTON.0 as i32) as u16 & 0x8000) != 0;
                if mbutton && !prev_mbutton {
                    if let Some(hit) = layout.hit_test(cursor.0, cursor.1, &windows, sw, sh) {
                        close_request = Some(hit);
                    }
                }
                prev_mbutton = mbutton;

                if (GetAsyncKeyState(1) as u16 & 0x8000) != 0 {
                    if let Some(hit) = layout.hit_test(cursor.0, cursor.1, &windows, sw, sh) {
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

            // 5. Apply navigation
            if !candidates.is_empty() {
                if let Some(pos) = jump_to {
                    let pos = pos.min(candidates.len() - 1);
                    selected = candidates[pos];
                    spring.set_target(pos as f64);
                }
                if step != 0 {
                    let (idx, position) = move_within(&candidates, selected, step);
                    selected = idx;
                    spring.set_target(position as f64);
                }
            }

            // 6. Close a window: post WM_CLOSE and remove its card, keeping
            // the picker open (like the Windows switcher).
            if let Some(idx) = close_request.filter(|&i| i < windows.len()) {
                unsafe {
                    let _ = PostMessageW(windows[idx].hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                windows.remove(idx);
                bitmaps.remove(idx);
                icons.remove(idx);
                raw_images.remove(idx);
                if windows.is_empty() {
                    commit_selection = false;
                    break;
                }
                if selected > idx || selected >= windows.len() {
                    selected = selected.saturating_sub(1);
                }
                candidates = layout.visible_indices(&windows);
                if candidates.is_empty() {
                    // e.g. the search filter now matches nothing; keep the picker open.
                    spring.set_target(0.0);
                } else {
                    let pos = candidates
                        .iter()
                        .position(|&i| i >= selected)
                        .unwrap_or(candidates.len() - 1);
                    selected = candidates[pos];
                    spring.set_target(pos as f64);
                }
                last_activity = Instant::now();
            }

            // Idle watchdog: guards against a picker stranded by a missed
            // key-up or a touchpad that stopped reporting. It only counts
            // time with no navigation, typing or mouse movement, so browsing
            // or searching is never cut off. Preview mode was opened
            // deliberately and is always dismissable, so it has no timeout.
            if spring.target != last_target
                || cursor_moved
                || wheel_steps != 0
                || search_query.len() != last_query_len
            {
                last_target = spring.target;
                last_query_len = search_query.len();
                last_activity = Instant::now();
            }
            if mode != PickerMode::Preview && last_activity.elapsed() > PICKER_IDLE_TIMEOUT {
                commit_selection = false;
                break;
            }

            // 7. Update physics & layout
            spring.update(dt);
            if reduce_motion {
                spring.snap_to(spring.target);
            }
            if ((mode == PickerMode::Touchpad && fnav_armed) || mode == PickerMode::AltTab)
                && !candidates.is_empty()
            {
                let position = (spring.value.round().max(0.0) as usize).min(candidates.len() - 1);
                selected = candidates[position];
            }

            layout.update(spring.value as f32, selected);

            // 8. Render frame with smooth fade-in
            let alpha = if fade_dur > 0.0 {
                (show_start.elapsed().as_secs_f32() / fade_dur).min(1.0)
            } else {
                1.0
            };
            let eased_alpha = 1.0 - (1.0 - alpha).powi(3);

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
                    eased_alpha,
                );
            } else {
                self.d2d.clear(0.04, 0.05, 0.08, eased_alpha);
            }

            // Light dim over the blurred desktop so cards and text stand out.
            self.d2d.fill_rect(
                D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                },
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.22 * eased_alpha,
                },
            );

            // Hint bar, sized to its text
            let pill_w = (self.d2d.measure(&hint_text, 12.0, 400, crate::render::d2d::Font::Text)
                + 40.0)
                .clamp(260.0, sw - 40.0);
            let pill_rect = D2D_RECT_F {
                left: (sw - pill_w) * 0.5,
                top: 14.0,
                right: (sw + pill_w) * 0.5,
                bottom: 40.0,
            };
            draw_liquid_glass(&self.d2d, pill_rect, 13.0, accent, false);
            self.d2d.text(
                &hint_text,
                pill_rect,
                D2D1_COLOR_F {
                    r: 0.80,
                    g: 0.82,
                    b: 0.86,
                    a: eased_alpha,
                },
                12.0,
                400,
                crate::render::d2d::Font::Text,
                crate::render::d2d::Align::Center,
            );

            layout.render(&self.d2d, &windows, &bitmaps, &icons, accent, sw, sh);

            let _ = self.d2d.end_draw();

            thread::sleep(Duration::from_millis(7));
        }

        self.hide();
        if !commit_selection || selected >= windows.len() {
            return None;
        }
        // Map back to the caller's list; frames are only reusable if the
        // current window (index 0) is still the first card.
        let target_hwnd = windows[selected].hwnd;
        let original_idx = original.iter().position(|w| w.hwnd == target_hwnd)?;
        let curr_img = if windows[0].hwnd == original[0].hwnd {
            raw_images.first().cloned().flatten()
        } else {
            None
        };
        let next_img = raw_images.get(selected).cloned().flatten();
        Some((original_idx, curr_img, next_img))
    }
}

/// Clears the "picker active" flag on every exit path so the thumbnail
/// refresh thread always stops.
struct StopFlagGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for StopFlagGuard {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

struct PickerKeysGuard;
impl Drop for PickerKeysGuard {
    fn drop(&mut self) {
        crate::input::keyboard_hook::set_picker_mode(false);
    }
}

/// Mouse-wheel delta accumulated by the overlay window procedure.
static WHEEL_ACCUM: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Whole wheel notches since the last call (+1 = scroll down = next card).
/// Partial deltas from smooth-scrolling touchpads are carried over.
fn take_wheel_steps() -> i32 {
    use std::sync::atomic::Ordering;
    let acc = WHEEL_ACCUM.load(Ordering::SeqCst);
    let notches = acc / 120;
    if notches != 0 {
        WHEEL_ACCUM.fetch_sub(notches * 120, Ordering::SeqCst);
    }
    -notches
}

fn has_visible_content(img: &ImageBuffer) -> bool {
    img.pixels
        .chunks_exact(16)
        .any(|px| px[0] > 15 || px[1] > 15 || px[2] > 15)
}

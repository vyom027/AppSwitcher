use crate::window::foreground::is_own_window;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HALFTONE, HDC, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, IsIconic, IsWindowVisible, IsZoomed, SystemParametersInfoW, SM_CXSCREEN,
    SM_CYSCREEN, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

#[link(name = "user32")]
extern "system" {
    fn PrintWindow(hwnd: HWND, hdc: HDC, nflags: u32) -> BOOL;
}

#[derive(Debug, Clone)]
pub struct ImageBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA format
}

pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

pub fn work_area() -> (i32, i32, i32, i32) {
    let (sw, sh) = screen_size();
    let mut wx = 0;
    let mut wy = 0;
    let mut ww = sw;
    let mut wh = sh;

    unsafe {
        let mut rect = RECT::default();
        if SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let w = rect.right - rect.left;
            let h = rect.bottom - rect.top;
            if w > 0 && h > 0 {
                wx = rect.left;
                wy = rect.top;
                ww = w;
                wh = h;
            }
        }

        // Dynamic Top-Bar Detection (YASB, Zebar, GlazeWM, RetroBar, etc.):
        // If SPI_GETWORKAREA did not report a top bar (e.g. YASB with windows_app_bar: false or floating),
        // inspect visible top-level windows to auto-detect any top bar docked to screen top.
        if wy == 0 {
            let mut detected_top_bar_h: i32 = 0;

            unsafe extern "system" fn enum_top_bar_proc(
                hwnd: HWND,
                lparam: windows::Win32::Foundation::LPARAM,
            ) -> BOOL {
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowLongW, GetWindowRect, IsWindowVisible, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
                    WS_EX_TOPMOST,
                };

                if !IsWindowVisible(hwnd).as_bool() || is_own_window(hwnd) {
                    return BOOL(1);
                }

                let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                let mut wr = RECT::default();
                if GetWindowRect(hwnd, &mut wr).is_ok() {
                    let w = wr.right - wr.left;
                    let h = wr.bottom - wr.top;
                    let (sw, _) = screen_size();

                    // Check if window is docked to top edge:
                    // top == 0, spans most of monitor width (>= 75%), and height between 16px and 120px
                    if wr.top == 0 && wr.left <= 4 && w >= (sw * 3 / 4) && h >= 16 && h <= 120 {
                        // Priority to topmost or toolwindow
                        if (ex_style & (WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0)) != 0 || h > 20 {
                            let out_h = lparam.0 as *mut i32;
                            if !out_h.is_null() && h > *out_h {
                                *out_h = h;
                            }
                        }
                    }
                }
                BOOL(1)
            }

            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(enum_top_bar_proc),
                windows::Win32::Foundation::LPARAM(&mut detected_top_bar_h as *mut i32 as isize),
            );

            if detected_top_bar_h > 0 {
                wy = detected_top_bar_h;
                wh = (sh - wy).max(100);
            }
        }
    }

    (wx, wy, ww, wh)
}

// ─── Frosted Glass Backdrop Generator & Cache ────────────────────────────────

static BACKDROP_CACHE: LazyLock<Mutex<Option<(Instant, ImageBuffer)>>> =
    LazyLock::new(|| Mutex::new(None));
static IS_PREWARMING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const BACKDROP_TTL: Duration = Duration::from_millis(1500);

pub fn prewarm_backdrop() {
    if IS_PREWARMING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    thread::spawn(|| {
        let (wx, wy, ww, wh) = work_area();
        if ww > 0 && wh > 0 {
            let bg = capture_frosted_backdrop(wx, wy, ww, wh);
            *BACKDROP_CACHE.lock() = Some((Instant::now(), bg));
        }
        IS_PREWARMING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

pub fn get_backdrop() -> ImageBuffer {
    let now = Instant::now();
    {
        let cache = BACKDROP_CACHE.lock();
        if let Some((ts, ref img)) = *cache {
            if now.duration_since(ts) < BACKDROP_TTL {
                return img.clone();
            }
        }
    }
    let (wx, wy, ww, wh) = work_area();
    let bg = capture_frosted_backdrop(wx, wy, ww, wh);
    *BACKDROP_CACHE.lock() = Some((now, bg.clone()));
    bg
}

fn box_blur_1d(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: usize, horizontal: bool) {
    let div = (radius * 2 + 1) as u32;

    if horizontal {
        for y in 0..h {
            let row_offset = y * w * 4;
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;

            // Seed sliding window
            for k in 0..=radius {
                let px = row_offset + k.min(w - 1) * 4;
                sum_r += src[px] as u32;
                sum_g += src[px + 1] as u32;
                sum_b += src[px + 2] as u32;
            }
            for _ in 0..radius {
                let px = row_offset;
                sum_r += src[px] as u32;
                sum_g += src[px + 1] as u32;
                sum_b += src[px + 2] as u32;
            }

            for x in 0..w {
                let out_idx = row_offset + x * 4;
                dst[out_idx] = (sum_r / div) as u8;
                dst[out_idx + 1] = (sum_g / div) as u8;
                dst[out_idx + 2] = (sum_b / div) as u8;
                dst[out_idx + 3] = 255;

                let add_x = (x + radius + 1).min(w - 1);
                let sub_x = x.saturating_sub(radius);

                let add_idx = row_offset + add_x * 4;
                let sub_idx = row_offset + sub_x * 4;

                sum_r = sum_r + src[add_idx] as u32 - src[sub_idx] as u32;
                sum_g = sum_g + src[add_idx + 1] as u32 - src[sub_idx + 1] as u32;
                sum_b = sum_b + src[add_idx + 2] as u32 - src[sub_idx + 2] as u32;
            }
        }
    } else {
        for x in 0..w {
            let col_offset = x * 4;
            let stride = w * 4;
            let mut sum_r = 0u32;
            let mut sum_g = 0u32;
            let mut sum_b = 0u32;

            for k in 0..=radius {
                let py = k.min(h - 1) * stride + col_offset;
                sum_r += src[py] as u32;
                sum_g += src[py + 1] as u32;
                sum_b += src[py + 2] as u32;
            }
            for _ in 0..radius {
                let py = col_offset;
                sum_r += src[py] as u32;
                sum_g += src[py + 1] as u32;
                sum_b += src[py + 2] as u32;
            }

            for y in 0..h {
                let out_idx = y * stride + col_offset;
                dst[out_idx] = (sum_r / div) as u8;
                dst[out_idx + 1] = (sum_g / div) as u8;
                dst[out_idx + 2] = (sum_b / div) as u8;
                dst[out_idx + 3] = 255;

                let add_y = (y + radius + 1).min(h - 1);
                let sub_y = y.saturating_sub(radius);

                let add_idx = add_y * stride + col_offset;
                let sub_idx = sub_y * stride + col_offset;

                sum_r = sum_r + src[add_idx] as u32 - src[sub_idx] as u32;
                sum_g = sum_g + src[add_idx + 1] as u32 - src[sub_idx + 1] as u32;
                sum_b = sum_b + src[add_idx + 2] as u32 - src[sub_idx + 2] as u32;
            }
        }
    }
}

/// Fast downsampled frosted backdrop capture with box blur + dark tint
pub fn capture_frosted_backdrop(wx: i32, wy: i32, ww: i32, wh: i32) -> ImageBuffer {
    let scale = 4;
    let bw = (ww / scale).max(1) as u32;
    let bh = (wh / scale).max(1) as u32;

    unsafe {
        let hdc_screen = GetDC(GetDesktopWindow());
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, bw as i32, bh as i32);
        let old_obj = SelectObject(hdc_mem, hbm);

        let _ = SetStretchBltMode(hdc_mem, HALFTONE);
        let _ = StretchBlt(
            hdc_mem, 0, 0, bw as i32, bh as i32, hdc_screen, wx, wy, ww, wh, SRCCOPY,
        );

        SelectObject(hdc_mem, old_obj);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: bw as i32,
                biHeight: -(bh as i32), // Top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
        };

        let mut raw_pixels = vec![0u8; (bw * bh * 4) as usize];
        let lines = GetDIBits(
            hdc_mem,
            hbm,
            0,
            bh,
            Some(raw_pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(GetDesktopWindow(), hdc_screen);

        if lines > 0 {
            // Swap BGR to RGB
            for chunk in raw_pixels.chunks_exact_mut(4) {
                let b = chunk[0];
                let r = chunk[2];
                chunk[0] = r;
                chunk[2] = b;
                chunk[3] = 255;
            }

            // 3-pass separable box blur (simulates Gaussian blur in < 1ms)
            let mut temp = vec![0u8; raw_pixels.len()];
            let w = bw as usize;
            let h = bh as usize;

            box_blur_1d(&raw_pixels, &mut temp, w, h, 4, true);
            box_blur_1d(&temp, &mut raw_pixels, w, h, 4, false);
            box_blur_1d(&raw_pixels, &mut temp, w, h, 4, true);
            box_blur_1d(&temp, &mut raw_pixels, w, h, 4, false);

            // Preserve the desktop's color and shape under the glass; the old
            // 55% near-black wash crushed detail and made every picker feel muddy.
            for chunk in raw_pixels.chunks_exact_mut(4) {
                chunk[0] = ((chunk[0] as u32 * 78 + 20 * 22) / 100) as u8;
                chunk[1] = ((chunk[1] as u32 * 78 + 27 * 22) / 100) as u8;
                chunk[2] = ((chunk[2] as u32 * 78 + 42 * 22) / 100) as u8;
                chunk[3] = 255;
            }

            return ImageBuffer {
                width: bw,
                height: bh,
                pixels: raw_pixels,
            };
        }
    }

    // High-end fallback dark frosted tint
    let mut fallback = vec![0u8; (bw * bh * 4) as usize];
    for chunk in fallback.chunks_exact_mut(4) {
        chunk[0] = 24;
        chunk[1] = 31;
        chunk[2] = 47;
        chunk[3] = 255;
    }
    ImageBuffer {
        width: bw,
        height: bh,
        pixels: fallback,
    }
}

// ─── Window Frame Cache & Parallel Capture ───────────────────────────────────

pub static OVERLAY_VISIBLE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
pub static GDI_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

static FRAME_CACHE: LazyLock<Mutex<HashMap<isize, (Instant, ImageBuffer)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const FRAME_TTL: Duration = Duration::from_millis(3000);
const FRAME_RETENTION: Duration = Duration::from_secs(10);
const MAX_FRAME_CACHE_BYTES: usize = 48 * 1024 * 1024;
const MAX_FRAME_CACHE_ENTRIES: usize = 5;

fn cache_frame(hwnd: isize, captured_at: Instant, image: ImageBuffer) {
    let mut cache = FRAME_CACHE.lock();
    cache.retain(|_, (ts, _)| captured_at.duration_since(*ts) < FRAME_RETENTION);
    cache.insert(hwnd, (captured_at, image));

    while cache.len() > MAX_FRAME_CACHE_ENTRIES
        || (cache.len() > 1
            && cache.values().map(|(_, img)| img.pixels.len()).sum::<usize>()
                > MAX_FRAME_CACHE_BYTES)
    {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (ts, _))| *ts)
            .map(|(hwnd, _)| *hwnd)
        {
            cache.remove(&oldest);
        }
    }
}

static IS_PREWARMING_WINDOWS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn prewarm_windows(hwnds: &[HWND]) {
    if IS_PREWARMING_WINDOWS.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let list: Vec<usize> = hwnds.iter().map(|h| h.0 as usize).collect();
    thread::spawn(move || {
        let now = Instant::now();
        for h in list {
            let hwnd = HWND(h as *mut _);
            let needs_cap = {
                let cache = FRAME_CACHE.lock();
                cache
                    .get(&(h as isize))
                    .map_or(true, |(ts, _)| now.duration_since(*ts) >= FRAME_TTL)
            };
            if needs_cap {
                if let Some(img) = capture_window_image_uncached(hwnd) {
                    cache_frame(h as isize, Instant::now(), img);
                }
            }
        }
        IS_PREWARMING_WINDOWS.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

pub fn create_placeholder(_title: &str, width: u32, height: u32) -> ImageBuffer {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 16; // R
        chunk[1] = 18; // G
        chunk[2] = 22; // B - Sleek neutral dark obsidian glass (no bright blue!)
        chunk[3] = 255; // A
    }

    ImageBuffer {
        width,
        height,
        pixels,
    }
}

pub fn grab_screen_rect(x: i32, y: i32, w: i32, h: i32) -> Option<ImageBuffer> {
    if w <= 0 || h <= 0 {
        return None;
    }
    let _lock = GDI_LOCK.lock();
    unsafe {
        let hdc_screen = GetDC(GetDesktopWindow());
        if hdc_screen.0.is_null() {
            return None;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
        let old_obj = SelectObject(hdc_mem, hbm);

        let _ = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, x, y, SRCCOPY);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
        };

        SelectObject(hdc_mem, old_obj);

        let mut raw_pixels = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(
            hdc_mem,
            hbm,
            0,
            h as u32,
            Some(raw_pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(GetDesktopWindow(), hdc_screen);

        if lines > 0 {
            for chunk in raw_pixels.chunks_exact_mut(4) {
                let b = chunk[0];
                let r = chunk[2];
                chunk[0] = r;
                chunk[2] = b;
                chunk[3] = 255;
            }
            Some(ImageBuffer {
                width: w as u32,
                height: h as u32,
                pixels: raw_pixels,
            })
        } else {
            None
        }
    }
}

fn crop_inner(
    img: &ImageBuffer,
    crop_x: u32,
    crop_y: u32,
    crop_w: u32,
    crop_h: u32,
) -> ImageBuffer {
    let mut out = vec![0u8; (crop_w * crop_h * 4) as usize];
    let src_stride = (img.width * 4) as usize;
    let dst_stride = (crop_w * 4) as usize;

    for row in 0..crop_h {
        let sy = (crop_y + row) as usize;
        if sy >= img.height as usize {
            break;
        }
        let sx = (crop_x * 4) as usize;
        if sx < src_stride {
            let copy_bytes = (crop_w * 4) as usize;
            let available = src_stride - sx;
            let len = copy_bytes.min(available);
            let row_src = &img.pixels[sy * src_stride + sx..sy * src_stride + sx + len];
            let row_dst = &mut out[(row as usize) * dst_stride..(row as usize) * dst_stride + len];
            row_dst.copy_from_slice(row_src);
        }
    }

    ImageBuffer {
        width: crop_w,
        height: crop_h,
        pixels: out,
    }
}

fn compose_over_backdrop(win: &ImageBuffer, dx: i32, dy: i32, ww: u32, wh: u32) -> ImageBuffer {
    let mut canvas = vec![0u8; (ww * wh * 4) as usize];
    for chunk in canvas.chunks_exact_mut(4) {
        chunk[0] = 16;
        chunk[1] = 18;
        chunk[2] = 24;
        chunk[3] = 255;
    }

    let win_w = win.width as i32;
    let win_h = win.height as i32;
    let dst_stride = (ww * 4) as usize;
    let src_stride = (win.width * 4) as usize;

    for y in 0..win_h {
        let cy = dy + y;
        if cy < 0 || cy >= wh as i32 {
            continue;
        }
        for x in 0..win_w {
            let cx = dx + x;
            if cx < 0 || cx >= ww as i32 {
                continue;
            }
            let si = (y as usize) * src_stride + (x as usize) * 4;
            let di = (cy as usize) * dst_stride + (cx as usize) * 4;
            canvas[di..di + 4].copy_from_slice(&win.pixels[si..si + 4]);
        }
    }

    ImageBuffer {
        width: ww,
        height: wh,
        pixels: canvas,
    }
}

pub fn capture_window_image(hwnd: HWND) -> Option<ImageBuffer> {
    capture_window_image_timed(hwnd, Duration::from_millis(60))
}

pub fn capture_window_image_timed(hwnd: HWND, timeout: Duration) -> Option<ImageBuffer> {
    let raw = hwnd.0 as isize;
    let now = Instant::now();
    {
        let cache = FRAME_CACHE.lock();
        if let Some((ts, ref img)) = cache.get(&raw) {
            if now.duration_since(*ts) < FRAME_TTL {
                return Some(img.clone());
            }
        }
    }

    let ov_vis = OVERLAY_VISIBLE.load(std::sync::atomic::Ordering::Relaxed);
    let is_fg = unsafe { GetForegroundWindow() == hwnd && !IsIconic(hwnd).as_bool() };
    if is_fg && !ov_vis {
        let (wx, wy, ww, wh) = work_area();
        if let Some(buf) = grab_screen_rect(wx, wy, ww, wh) {
            cache_frame(raw, now, buf.clone());
            return Some(buf);
        }
    }

    // Run PrintWindow on a worker thread with strict timeout to prevent ANY UI thread freeze
    let (tx, rx) = crossbeam_channel::bounded::<Option<ImageBuffer>>(1);
    let h_raw = hwnd.0 as usize;
    thread::spawn(move || {
        let h = HWND(h_raw as *mut _);
        let res = capture_window_image_uncached(h);
        let _ = tx.send(res);
    });

    match rx.recv_timeout(timeout) {
        Ok(Some(buf)) => {
            cache_frame(raw, Instant::now(), buf.clone());
            Some(buf)
        }
        Ok(None) | Err(_) => {
            // Timed out or unresponsive app: fallback to stale cached frame or placeholder
            let stale = {
                let cache = FRAME_CACHE.lock();
                cache.get(&raw).map(|(_, img)| img.clone())
            };
            stale.or_else(|| {
                let title = get_window_title_safe(hwnd);
                Some(create_placeholder(&title, 480, 300))
            })
        }
    }
}

fn get_window_title_safe(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let read = GetWindowTextW(hwnd, &mut buf);
            if read > 0 {
                return String::from_utf16_lossy(&buf[..read as usize]);
            }
        }
    }
    String::new()
}

pub fn capture_window_image_uncached(hwnd: HWND) -> Option<ImageBuffer> {
    if is_own_window(hwnd) {
        return Some(create_placeholder("AppSwitcher", 480, 300));
    }

    let (wx, wy, ww, wh) = work_area();
    if ww <= 0 || wh <= 0 {
        return Some(create_placeholder("", 480, 300));
    }

    unsafe {
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return Some(create_placeholder("", 480, 300));
        }

        let w = (rect.right - rect.left) as i32;
        let h = (rect.bottom - rect.top) as i32;
        if w <= 0 || h <= 0 {
            return Some(create_placeholder("", 480, 300));
        }

        let is_iconic = IsIconic(hwnd).as_bool();
        let is_fg = GetForegroundWindow() == hwnd;
        let is_maximized = IsZoomed(hwnd).as_bool();
        let ov_vis = OVERLAY_VISIBLE.load(std::sync::atomic::Ordering::Relaxed);

        // 1. Fast screen DC grab ONLY if foreground and our overlay is NOT currently shown!
        if is_fg && !is_iconic && !ov_vis {
            if let Some(buf) = grab_screen_rect(wx, wy, ww, wh) {
                return Some(buf);
            }
        }

        if windows::Win32::UI::WindowsAndMessaging::IsHungAppWindow(hwnd).as_bool() {
            let title = get_window_title_safe(hwnd);
            return Some(create_placeholder(
                &title,
                w.max(320) as u32,
                h.max(200) as u32,
            ));
        }

        // 2. Background or occluded window: PrintWindow using Screen-compatible DC (guarantees 32-bit color!)
        let hdc_screen = GetDC(GetDesktopWindow());
        if !hdc_screen.0.is_null() {
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
            let old_obj = SelectObject(hdc_mem, hbm);

            // Try PW_RENDERFULLCONTENT (2) first; fallback to standard PrintWindow (0)
            let mut printed = PrintWindow(hwnd, hdc_mem, 2).as_bool();
            if !printed {
                printed = PrintWindow(hwnd, hdc_mem, 0).as_bool();
            }

            if printed {
                let mut bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        biSizeImage: 0,
                        biXPelsPerMeter: 0,
                        biYPelsPerMeter: 0,
                        biClrUsed: 0,
                        biClrImportant: 0,
                    },
                    bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
                };

                SelectObject(hdc_mem, old_obj);

                let mut raw_pixels = vec![0u8; (w * h * 4) as usize];
                let lines = GetDIBits(
                    hdc_mem,
                    hbm,
                    0,
                    h as u32,
                    Some(raw_pixels.as_mut_ptr() as *mut _),
                    &mut bmi,
                    DIB_RGB_COLORS,
                );

                let _ = DeleteObject(hbm);
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(GetDesktopWindow(), hdc_screen);

                if lines > 0 {
                    let mut non_black = false;
                    for chunk in raw_pixels.chunks_exact_mut(4) {
                        let b = chunk[0];
                        let g = chunk[1];
                        let r = chunk[2];
                        if r > 10 || g > 10 || b > 10 {
                            non_black = true;
                        }
                        chunk[0] = r;
                        chunk[2] = b;
                        chunk[3] = 255;
                    }

                    if non_black {
                        let raw_buf = ImageBuffer {
                            width: w as u32,
                            height: h as u32,
                            pixels: raw_pixels,
                        };

                        if is_maximized {
                            if (w as u32) >= ww as u32 && (h as u32) >= wh as u32 {
                                let crop_x = ((w as u32 - ww as u32) / 2).min(16);
                                let crop_y = ((h as u32 - wh as u32) / 2).min(16);
                                return Some(crop_inner(
                                    &raw_buf, crop_x, crop_y, ww as u32, wh as u32,
                                ));
                            }
                            return Some(raw_buf);
                        } else {
                            let dx = rect.left - wx;
                            let dy = rect.top - wy;
                            return Some(compose_over_backdrop(
                                &raw_buf, dx, dy, ww as u32, wh as u32,
                            ));
                        }
                    }
                }
            } else {
                SelectObject(hdc_mem, old_obj);
                let _ = DeleteObject(hbm);
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(GetDesktopWindow(), hdc_screen);
            }
        }

        // Some GPU-composited windows report success from PrintWindow while
        // returning an all-black bitmap. When the window is actually visible,
        // fall back to its current desktop pixels rather than showing a black
        // preview. This path runs before the switcher overlay is revealed.
        if !is_iconic && IsWindowVisible(hwnd).as_bool() {
            let screen = grab_screen_rect(rect.left, rect.top, w, h);
            if let Some(img) = screen {
                let has_content = img
                    .pixels
                    .chunks_exact(4)
                    .any(|p| p[0] > 12 || p[1] > 12 || p[2] > 12);
                if has_content {
                    let dx = rect.left - wx;
                    let dy = rect.top - wy;
                    return Some(compose_over_backdrop(&img, dx, dy, ww as u32, wh as u32));
                }
            }
        }

        None
    }
}

/// Capture multiple windows in parallel with strict total deadline
pub fn capture_windows_parallel(hwnds: &[HWND]) -> Vec<Option<ImageBuffer>> {
    let mut results = vec![None; hwnds.len()];
    let mut missing_indices = Vec::new();

    let now = Instant::now();
    {
        let cache = FRAME_CACHE.lock();
        for (i, &hwnd) in hwnds.iter().enumerate() {
            let raw = hwnd.0 as isize;
            if let Some((ts, ref img)) = cache.get(&raw) {
                if now.duration_since(*ts) < FRAME_TTL {
                    results[i] = Some(img.clone());
                    continue;
                }
            }
            missing_indices.push(i);
        }
    }

    if missing_indices.is_empty() {
        return results;
    }

    // Capture missing in parallel across threads with 120ms total deadline
    let (tx, rx) = crossbeam_channel::unbounded::<(usize, Option<ImageBuffer>)>();
    for &idx in &missing_indices {
        let hwnd = hwnds[idx];
        let tx_w = tx.clone();
        let h_raw = hwnd.0 as usize;
        thread::spawn(move || {
            let h = HWND(h_raw as *mut _);
            let img = capture_window_image_uncached(h);
            let _ = tx_w.send((idx, img));
        });
    }
    drop(tx);

    let deadline = Instant::now() + Duration::from_millis(120);
    while let Some(rem) = deadline.checked_duration_since(Instant::now()) {
        if rem.is_zero() {
            break;
        }
        match rx.recv_timeout(rem) {
            Ok((idx, Some(img))) => {
                cache_frame(hwnds[idx].0 as isize, Instant::now(), img.clone());
                results[idx] = Some(img);
            }
            Ok((idx, None)) => {
                let title = get_window_title_safe(hwnds[idx]);
                results[idx] = Some(create_placeholder(&title, 480, 300));
            }
            Err(_) => break,
        }
    }

    // Fill any that timed out with stale cache or instant placeholder
    for &idx in &missing_indices {
        if results[idx].is_none() {
            let hwnd = hwnds[idx];
            let stale = {
                let cache = FRAME_CACHE.lock();
                cache.get(&(hwnd.0 as isize)).map(|(_, img)| img.clone())
            };
            results[idx] = stale.or_else(|| {
                let title = get_window_title_safe(hwnd);
                Some(create_placeholder(&title, 480, 300))
            });
        }
    }

    results
}

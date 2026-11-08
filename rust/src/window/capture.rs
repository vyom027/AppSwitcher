use crate::window::foreground::is_own_window;
use std::mem::size_of;
use windows::Win32::Foundation::{BOOL, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    GetDIBits, GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HDC, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetSystemMetrics, GetWindowRect, SystemParametersInfoW,
    SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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
    unsafe {
        (
            GetSystemMetrics(SM_CXSCREEN),
            GetSystemMetrics(SM_CYSCREEN),
        )
    }
}

pub fn work_area() -> (i32, i32, i32, i32) {
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
                return (rect.left, rect.top, w, h);
            }
        }
        let (sw, sh) = screen_size();
        (0, 0, sw, sh)
    }
}

/// Fallback placeholder when a window cannot be captured (e.g. elevated or protected)
pub fn create_placeholder(_title: &str, width: u32, height: u32) -> ImageBuffer {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    // Dark sleek card fill (#181B24)
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = 24;  // R
        chunk[1] = 27;  // G
        chunk[2] = 36;  // B
        chunk[3] = 255; // A
    }

    // Add top accent bar
    let bar_height = (height / 20).max(4);
    for y in 0..bar_height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;
            pixels[idx] = 150;     // R
            pixels[idx + 1] = 205; // G
            pixels[idx + 2] = 255; // B
            pixels[idx + 3] = 255;
        }
    }

    ImageBuffer {
        width,
        height,
        pixels,
    }
}

pub fn capture_window_image(hwnd: HWND) -> Option<ImageBuffer> {
    if is_own_window(hwnd) {
        return Some(create_placeholder("AppSwitcher", 480, 300));
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

        let is_fg = GetForegroundWindow() == hwnd;

        // Fast-path: if foreground and visible, grab straight from screen DC
        if is_fg {
            let hdc_screen = GetDC(GetDesktopWindow());
            let hdc_mem = CreateCompatibleDC(hdc_screen);
            let hbm = CreateCompatibleBitmap(hdc_screen, w, h);
            let old_obj = SelectObject(hdc_mem, hbm);

            let _ = BitBlt(hdc_mem, 0, 0, w, h, hdc_screen, rect.left, rect.top, SRCCOPY);

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

            SelectObject(hdc_mem, old_obj);
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
                return Some(ImageBuffer {
                    width: w as u32,
                    height: h as u32,
                    pixels: raw_pixels,
                });
            }
        }

        // Secondary path: PrintWindow (PW_RENDERFULLCONTENT = 2)
        let hdc_window = GetWindowDC(hwnd);
        if !hdc_window.0.is_null() {
            let hdc_mem = CreateCompatibleDC(hdc_window);
            let hbm = CreateCompatibleBitmap(hdc_window, w, h);
            let old_obj = SelectObject(hdc_mem, hbm);

            let printed = PrintWindow(hwnd, hdc_mem, 2).as_bool();
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

                SelectObject(hdc_mem, old_obj);
                let _ = DeleteObject(hbm);
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(hwnd, hdc_window);

                if lines > 0 {
                    for chunk in raw_pixels.chunks_exact_mut(4) {
                        let b = chunk[0];
                        let r = chunk[2];
                        chunk[0] = r;
                        chunk[2] = b;
                        chunk[3] = 255;
                    }

                    return Some(ImageBuffer {
                        width: w as u32,
                        height: h as u32,
                        pixels: raw_pixels,
                    });
                }
            } else {
                SelectObject(hdc_mem, old_obj);
                let _ = DeleteObject(hbm);
                let _ = DeleteDC(hdc_mem);
                let _ = ReleaseDC(hwnd, hdc_window);
            }
        }

        // Safe fallback placeholder - guarantees switcher never aborts!
        Some(create_placeholder("", w as u32, h as u32))
    }
}

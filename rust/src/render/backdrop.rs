use std::mem::size_of;
use std::time::{Duration, Instant};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
    GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

#[derive(Debug, Clone)]
pub struct FrostedBackdrop {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA format
}

static BACKDROP_CACHE: parking_lot::Mutex<Option<(Instant, FrostedBackdrop)>> =
    parking_lot::Mutex::new(None);
const BACKDROP_TTL: Duration = Duration::from_millis(1500);

pub fn get_frosted_backdrop(sw: u32, sh: u32) -> FrostedBackdrop {
    let now = Instant::now();
    {
        let cache = BACKDROP_CACHE.lock();
        if let Some((ts, ref bg)) = *cache {
            if now.duration_since(ts) < BACKDROP_TTL && bg.width == sw && bg.height == sh {
                return bg.clone();
            }
        }
    }

    let bg = generate_frosted_backdrop(sw, sh);
    *BACKDROP_CACHE.lock() = Some((now, bg.clone()));
    bg
}

fn generate_frosted_backdrop(sw: u32, sh: u32) -> FrostedBackdrop {
    let scale = 4;
    let qw = (sw / scale).max(1);
    let qh = (sh / scale).max(1);

    unsafe {
        let hwnd_desktop = GetDesktopWindow();
        let hdc_screen = GetDC(hwnd_desktop);
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, qw as i32, qh as i32);
        let old_obj = SelectObject(hdc_mem, hbm);

        // Capture downscaled screen
        windows::Win32::Graphics::Gdi::SetStretchBltMode(
            hdc_mem,
            windows::Win32::Graphics::Gdi::HALFTONE,
        );

        let _ = windows::Win32::Graphics::Gdi::StretchBlt(
            hdc_mem,
            0,
            0,
            qw as i32,
            qh as i32,
            hdc_screen,
            0,
            0,
            sw as i32,
            sh as i32,
            SRCCOPY,
        );

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: qw as i32,
                biHeight: -(qh as i32), // Top-down
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

        let mut small_pixels = vec![0u8; (qw * qh * 4) as usize];
        let _ = GetDIBits(
            hdc_mem,
            hbm,
            0,
            qh,
            Some(small_pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(hdc_mem, old_obj);
        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(hwnd_desktop, hdc_screen);

        // Simple box blur on the small buffer (fast and smooth)
        blur_rgba(&mut small_pixels, qw as usize, qh as usize, 3);

        // Upscale back with dim overlay
        let mut full_pixels = vec![0u8; (sw * sh * 4) as usize];
        for y in 0..sh {
            let sy = (y / scale).min(qh - 1) as usize;
            for x in 0..sw {
                let sx = (x / scale).min(qw - 1) as usize;
                let s_idx = (sy * qw as usize + sx) * 4;
                let d_idx = (y as usize * sw as usize + x as usize) * 4;

                let r = small_pixels[s_idx + 2] as f32;
                let g = small_pixels[s_idx + 1] as f32;
                let b = small_pixels[s_idx] as f32;

                // Dim blend with dark void (12, 14, 20)
                let dim_r = (r * 0.55 + 12.0 * 0.45) as u8;
                let dim_g = (g * 0.55 + 14.0 * 0.45) as u8;
                let dim_b = (b * 0.55 + 20.0 * 0.45) as u8;

                full_pixels[d_idx] = dim_r;
                full_pixels[d_idx + 1] = dim_g;
                full_pixels[d_idx + 2] = dim_b;
                full_pixels[d_idx + 3] = 255;
            }
        }

        FrostedBackdrop {
            width: sw,
            height: sh,
            pixels: full_pixels,
        }
    }
}

fn blur_rgba(pixels: &mut [u8], width: usize, height: usize, radius: usize) {
    let mut temp = pixels.to_vec();

    // Horizontal pass
    for y in 0..height {
        for x in 0..width {
            let mut r = 0usize;
            let mut g = 0usize;
            let mut b = 0usize;
            let mut count = 0usize;

            let x_start = x.saturating_sub(radius);
            let x_end = (x + radius).min(width - 1);

            for kx in x_start..=x_end {
                let idx = (y * width + kx) * 4;
                b += pixels[idx] as usize;
                g += pixels[idx + 1] as usize;
                r += pixels[idx + 2] as usize;
                count += 1;
            }

            let d_idx = (y * width + x) * 4;
            temp[d_idx] = (b / count) as u8;
            temp[d_idx + 1] = (g / count) as u8;
            temp[d_idx + 2] = (r / count) as u8;
            temp[d_idx + 3] = 255;
        }
    }

    // Vertical pass
    for y in 0..height {
        let y_start = y.saturating_sub(radius);
        let y_end = (y + radius).min(height - 1);

        for x in 0..width {
            let mut r = 0usize;
            let mut g = 0usize;
            let mut b = 0usize;
            let mut count = 0usize;

            for ky in y_start..=y_end {
                let idx = (ky * width + x) * 4;
                b += temp[idx] as usize;
                g += temp[idx + 1] as usize;
                r += temp[idx + 2] as usize;
                count += 1;
            }

            let d_idx = (y * width + x) * 4;
            pixels[d_idx] = (b / count) as u8;
            pixels[d_idx + 1] = (g / count) as u8;
            pixels[d_idx + 2] = (r / count) as u8;
            pixels[d_idx + 3] = 255;
        }
    }
}


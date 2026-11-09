use crate::window::capture::ImageBuffer;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::mem::size_of;
use std::sync::LazyLock;
use windows::core::PWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetClassLongPtrW, GetIconInfo, GetWindowThreadProcessId, SendMessageTimeoutW,
    GCLP_HICON, GCLP_HICONSM, HICON, ICONINFO, ICON_BIG, ICON_SMALL, SMTO_ABORTIFHUNG, WM_GETICON,
};

static ICON_CACHE: LazyLock<Mutex<HashMap<isize, ImageBuffer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn get_window_icon(hwnd: HWND) -> Option<ImageBuffer> {
    let raw = hwnd.0 as isize;
    {
        let cache = ICON_CACHE.lock();
        if let Some(img) = cache.get(&raw) {
            return Some(img.clone());
        }
    }

    let hicon = extract_hicon(hwnd);
    if let Some(icon) = hicon {
        if let Some(buf) = hicon_to_image(icon, 32, 32) {
            unsafe {
                let _ = DestroyIcon(icon);
            }
            ICON_CACHE.lock().insert(raw, buf.clone());
            return Some(buf);
        }
        unsafe {
            let _ = DestroyIcon(icon);
        }
    }

    None
}

fn extract_hicon(hwnd: HWND) -> Option<HICON> {
    unsafe {
        let mut result = 0usize;

        // 1. WM_GETICON (ICON_BIG)
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            100,
            Some(&mut result),
        );
        if ok.0 != 0 && result != 0 {
            return Some(HICON(result as *mut _));
        }

        // 2. WM_GETICON (ICON_SMALL)
        let ok = SendMessageTimeoutW(
            hwnd,
            WM_GETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            100,
            Some(&mut result),
        );
        if ok.0 != 0 && result != 0 {
            return Some(HICON(result as *mut _));
        }

        // 3. Class icon
        let h = GetClassLongPtrW(hwnd, GCLP_HICON);
        if h != 0 {
            return Some(HICON(h as *mut _));
        }

        let h_sm = GetClassLongPtrW(hwnd, GCLP_HICONSM);
        if h_sm != 0 {
            return Some(HICON(h_sm as *mut _));
        }

        // 4. Executable icon from process path
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 {
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut path_buf = [0u16; 1024];
                let mut size = path_buf.len() as u32;
                if QueryFullProcessImageNameW(
                    process,
                    PROCESS_NAME_WIN32,
                    PWSTR(path_buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    let mut large_icon = HICON::default();
                    let mut small_icon = HICON::default();
                    let extracted = ExtractIconExW(
                        PWSTR(path_buf.as_mut_ptr()),
                        0,
                        Some(&mut large_icon),
                        Some(&mut small_icon),
                        1,
                    );
                    if extracted > 0 {
                        if !large_icon.0.is_null() {
                            if !small_icon.0.is_null() {
                                let _ = DestroyIcon(small_icon);
                            }
                            return Some(large_icon);
                        } else if !small_icon.0.is_null() {
                            return Some(small_icon);
                        }
                    }
                }
            }
        }

        None
    }
}

fn hicon_to_image(hicon: HICON, target_w: u32, target_h: u32) -> Option<ImageBuffer> {
    unsafe {
        let mut icon_info = ICONINFO::default();
        if GetIconInfo(hicon, &mut icon_info).is_err() {
            return None;
        }

        let hdc_screen = GetDC(None);
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, target_w as i32, target_h as i32);
        let old_hbm = SelectObject(hdc_mem, hbm);

        // Draw icon into memory DC
        let _ = windows::Win32::UI::WindowsAndMessaging::DrawIconEx(
            hdc_mem,
            0,
            0,
            hicon,
            target_w as i32,
            target_h as i32,
            0,
            None,
            windows::Win32::UI::WindowsAndMessaging::DI_NORMAL,
        );

        SelectObject(hdc_mem, old_hbm);

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: target_w as i32,
                biHeight: -(target_h as i32),
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

        let mut raw_pixels = vec![0u8; (target_w * target_h * 4) as usize];
        let lines = GetDIBits(
            hdc_mem,
            hbm,
            0,
            target_h,
            Some(raw_pixels.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(hbm);
        let _ = DeleteDC(hdc_mem);
        let _ = ReleaseDC(None, hdc_screen);

        if !icon_info.hbmColor.0.is_null() {
            let _ = DeleteObject(icon_info.hbmColor);
        }
        if !icon_info.hbmMask.0.is_null() {
            let _ = DeleteObject(icon_info.hbmMask);
        }

        if lines > 0 {
            // Check alpha channel and swap BGR to RGBA
            let mut has_non_zero_alpha = false;
            for chunk in raw_pixels.chunks_exact(4) {
                if chunk[3] > 0 {
                    has_non_zero_alpha = true;
                    break;
                }
            }

            for chunk in raw_pixels.chunks_exact_mut(4) {
                let b = chunk[0];
                let r = chunk[2];
                chunk[0] = r;
                chunk[2] = b;
                if !has_non_zero_alpha {
                    // 24-bit icon: mask black background as transparent or opaque
                    chunk[3] = if chunk[0] == 0 && chunk[1] == 0 && chunk[2] == 0 {
                        0
                    } else {
                        255
                    };
                }
            }

            Some(ImageBuffer {
                width: target_w,
                height: target_h,
                pixels: raw_pixels,
            })
        } else {
            None
        }
    }
}

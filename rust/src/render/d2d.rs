#![allow(dead_code)]

use windows::core::{w, Interface};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Brush, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
};

pub struct D2DContext {
    pub factory: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub target: ID2D1HwndRenderTarget,
    pub rt: ID2D1RenderTarget,
    pub brush: ID2D1SolidColorBrush,
}

unsafe impl Send for D2DContext {}
unsafe impl Sync for D2DContext {}

impl D2DContext {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self, windows::core::Error> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                ..Default::default()
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let target = factory.CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            target.SetDpi(96.0, 96.0);
            let rt: ID2D1RenderTarget = target.cast()?;

            let brush = rt.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
                None,
            )?;

            Ok(Self {
                factory,
                dwrite,
                target,
                rt,
                brush,
            })
        }
    }

    pub fn resize(&self, width: u32, height: u32) {
        unsafe {
            let _ = self.target.Resize(&D2D_SIZE_U { width, height });
        }
    }

    pub fn begin_draw(&self) {
        unsafe {
            self.rt.BeginDraw();
        }
    }

    pub fn end_draw(&self) -> Result<(), windows::core::Error> {
        unsafe { self.rt.EndDraw(None, None) }
    }

    pub fn clear(&self, r: f32, g: f32, b: f32, a: f32) {
        unsafe {
            self.rt.Clear(Some(&D2D1_COLOR_F { r, g, b, a }));
        }
    }

    pub fn draw_rounded_rect(
        &self,
        rect: D2D_RECT_F,
        radius: f32,
        color: D2D1_COLOR_F,
        stroke_width: f32,
    ) {
        unsafe {
            self.brush.SetColor(&color);
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            self.rt
                .DrawRoundedRectangle(&rounded, &self.brush, stroke_width, None);
        }
    }

    pub fn fill_rounded_rect(&self, rect: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(&color);
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            self.rt.FillRoundedRectangle(&rounded, &self.brush);
        }
    }

    pub fn fill_rect(&self, rect: D2D_RECT_F, color: D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(&color);
            self.rt.FillRectangle(&rect, &self.brush);
        }
    }

    pub fn draw_text(
        &self,
        text: &str,
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
        font_size: f32,
        bold: bool,
        center: bool,
    ) {
        unsafe {
            let font_weight = if bold {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            if let Ok(format) = self.dwrite.CreateTextFormat(
                w!("Segoe UI Variable Display"),
                None,
                font_weight,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                w!("en-us"),
            ) {
                if center {
                    let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                } else {
                    let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                }

                let wide: Vec<u16> = text.encode_utf16().collect();
                self.brush.SetColor(&color);
                self.rt.DrawText(
                    &wide,
                    &format,
                    &rect,
                    &self.brush,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }
    }

    pub fn create_bitmap_from_rgba(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<ID2D1Bitmap, windows::core::Error> {
        unsafe {
            let size = D2D_SIZE_U { width, height };
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_R8G8B8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };

            self.rt
                .CreateBitmap(size, Some(pixels.as_ptr() as *const _), width * 4, &props)
        }
    }

    pub fn draw_bitmap(&self, bitmap: &ID2D1Bitmap, dest_rect: D2D_RECT_F, opacity: f32) {
        unsafe {
            self.rt.DrawBitmap(
                bitmap,
                Some(&dest_rect),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                None,
            );
        }
    }

    pub fn draw_bitmap_subrect(
        &self,
        bitmap: &ID2D1Bitmap,
        dest_rect: D2D_RECT_F,
        src_rect: D2D_RECT_F,
        opacity: f32,
    ) {
        unsafe {
            self.rt.DrawBitmap(
                bitmap,
                Some(&dest_rect),
                opacity,
                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                Some(&src_rect),
            );
        }
    }

    /// Pixel-perfect antialiased rounded rectangle filled with a scaled bitmap
    pub fn fill_rounded_bitmap(
        &self,
        bitmap: &ID2D1Bitmap,
        dest_rect: D2D_RECT_F,
        radius: f32,
        opacity: f32,
    ) {
        unsafe {
            let bmp_size = bitmap.GetSize();
            if bmp_size.width <= 0.0 || bmp_size.height <= 0.0 {
                return;
            }

            let brush_props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };

            let dest_w = dest_rect.right - dest_rect.left;
            let dest_h = dest_rect.bottom - dest_rect.top;
            if dest_w <= 0.0 || dest_h <= 0.0 {
                return;
            }

            let scale_x = dest_w / bmp_size.width;
            let scale_y = dest_h / bmp_size.height;

            let transform = windows::Foundation::Numerics::Matrix3x2 {
                M11: scale_x,
                M12: 0.0,
                M21: 0.0,
                M22: scale_y,
                M31: dest_rect.left,
                M32: dest_rect.top,
            };

            if let Ok(brush) = self.rt.CreateBitmapBrush(bitmap, Some(&brush_props), None) {
                brush.SetTransform(&transform);
                brush.SetOpacity(opacity);
                let rounded = D2D1_ROUNDED_RECT {
                    rect: dest_rect,
                    radiusX: radius,
                    radiusY: radius,
                };
                if let Ok(b) = brush.cast::<ID2D1Brush>() {
                    self.rt.FillRoundedRectangle(&rounded, &b);
                }
            }
        }
    }

    /// Luxury raycast-caliber ambient radial glow behind cards
    pub fn draw_glow(
        &self,
        rect: D2D_RECT_F,
        radius: f32,
        color: D2D1_COLOR_F,
        blur_passes: usize,
    ) {
        for i in 1..=blur_passes {
            let expansion = i as f32 * 4.0;
            let alpha = color.a * (0.28 / (i as f32).powf(1.15));
            let expanded_rect = D2D_RECT_F {
                left: rect.left - expansion,
                top: rect.top - expansion,
                right: rect.right + expansion,
                bottom: rect.bottom + expansion,
            };
            self.fill_rounded_rect(
                expanded_rect,
                radius + expansion * 0.4,
                D2D1_COLOR_F {
                    r: color.r,
                    g: color.g,
                    b: color.b,
                    a: alpha,
                },
            );
        }
    }

    pub fn set_transform(&self, transform: &windows::Foundation::Numerics::Matrix3x2) {
        unsafe {
            self.rt.SetTransform(transform);
        }
    }

    pub fn reset_transform(&self) {
        unsafe {
            let identity = windows::Foundation::Numerics::Matrix3x2 {
                M11: 1.0,
                M12: 0.0,
                M21: 0.0,
                M22: 1.0,
                M31: 0.0,
                M32: 0.0,
            };
            self.rt.SetTransform(&identity);
        }
    }
}

use windows::core::{w, Interface};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget,
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

pub struct D2DContext {
    pub factory: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub target: ID2D1HwndRenderTarget,
    pub rt: ID2D1RenderTarget,
}

unsafe impl Send for D2DContext {}
unsafe impl Sync for D2DContext {}

impl D2DContext {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self, windows::core::Error> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES::default();
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
            };

            let target = factory.CreateHwndRenderTarget(&rt_props, &hwnd_props)?;
            target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let rt: ID2D1RenderTarget = target.cast()?;

            Ok(Self {
                factory,
                dwrite,
                target,
                rt,
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

    pub fn solid_brush(&self, color: D2D1_COLOR_F) -> Result<ID2D1SolidColorBrush, windows::core::Error> {
        unsafe { self.rt.CreateSolidColorBrush(&color, None) }
    }

    pub fn draw_rounded_rect(
        &self,
        rect: D2D_RECT_F,
        radius: f32,
        color: D2D1_COLOR_F,
        stroke_width: f32,
    ) {
        unsafe {
            if let Ok(brush) = self.solid_brush(color) {
                let rounded = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: radius,
                    radiusY: radius,
                };
                self.rt
                    .DrawRoundedRectangle(&rounded, &brush, stroke_width, None);
            }
        }
    }

    pub fn fill_rounded_rect(&self, rect: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F) {
        unsafe {
            if let Ok(brush) = self.solid_brush(color) {
                let rounded = D2D1_ROUNDED_RECT {
                    rect,
                    radiusX: radius,
                    radiusY: radius,
                };
                self.rt.FillRoundedRectangle(&rounded, &brush);
            }
        }
    }

    pub fn draw_text(
        &self,
        text: &str,
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
        font_size: f32,
        bold: bool,
        centered: bool,
    ) {
        unsafe {
            let weight = if bold {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            let font_name = w!("Segoe UI");
            if let Ok(format) = self.dwrite.CreateTextFormat(
                font_name,
                None,
                weight,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STYLE_NORMAL,
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                w!("en-us"),
            ) {
                if centered {
                    let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                    let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                } else {
                    let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
                }

                if let Ok(brush) = self.solid_brush(color) {
                    let wide: Vec<u16> = text.encode_utf16().collect();
                    self.rt.DrawText(
                        &wide,
                        &format,
                        &rect,
                        &brush,
                        windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                        windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                    );
                }
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

    pub fn draw_glow(&self, rect: D2D_RECT_F, radius: f32, color: D2D1_COLOR_F, blur_passes: usize) {
        for i in 1..=blur_passes {
            let expansion = i as f32 * 2.5;
            let alpha = color.a * (0.35 / (i as f32).powf(1.1));
            let expanded_rect = D2D_RECT_F {
                left: rect.left - expansion,
                top: rect.top - expansion,
                right: rect.right + expansion,
                bottom: rect.bottom + expansion,
            };
            let c = D2D1_COLOR_F {
                r: color.r,
                g: color.g,
                b: color.b,
                a: alpha,
            };
            self.fill_rounded_rect(expanded_rect, radius + expansion * 0.5, c);
        }
    }
}

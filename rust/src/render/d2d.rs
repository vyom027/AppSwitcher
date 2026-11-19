#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap, ID2D1Brush, ID2D1Factory, ID2D1HwndRenderTarget,
    ID2D1PathGeometry, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ELLIPSE,
    ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_BRUSH_PROPERTIES, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_GAMMA_2_2,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_CLIP;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
};

/// Font roles for `D2DContext::text`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Font {
    /// Segoe UI Variable Text: body copy and controls.
    Text,
    /// Segoe UI Variable Display: large titles.
    Display,
    /// Segoe Fluent Icons (Windows 11) or Segoe MDL2 Assets (Windows 10).
    Icons,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Align {
    Leading,
    Center,
    Trailing,
}

/// Common Fluent icon glyphs (same code points in both icon fonts).
pub mod glyph {
    pub const CHEVRON_DOWN: char = '\u{E70D}';
    pub const CHEVRON_UP: char = '\u{E70E}';
    pub const CHEVRON_LEFT: char = '\u{E76B}';
    pub const CHEVRON_RIGHT: char = '\u{E76C}';
    pub const CLOSE: char = '\u{E8BB}';
    pub const REFRESH: char = '\u{E72C}';
    pub const WARNING: char = '\u{E7BA}';
    pub const CHECK: char = '\u{E73E}';
    pub const PLAY: char = '\u{E768}';
    pub const VIEW_ALL: char = '\u{E8A9}';
    pub const PERSONALIZE: char = '\u{E771}';
    pub const SPEED: char = '\u{EC4A}';
    pub const HIDE: char = '\u{ED1A}';
    pub const VIEW: char = '\u{E890}';
    pub const KEYBOARD: char = '\u{E765}';
    pub const SEARCH: char = '\u{E721}';
    pub const SETTINGS: char = '\u{E713}';
    pub const POWER: char = '\u{E7E8}';
    pub const FOLDER: char = '\u{E8B7}';
    pub const INFO: char = '\u{E946}';
    pub const TOUCH: char = '\u{E7C9}';
    pub const COLOR: char = '\u{E790}';
}

type FormatKey = (Font, u32, u32, Align, bool);

pub struct D2DContext {
    pub factory: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub target: ID2D1HwndRenderTarget,
    pub rt: ID2D1RenderTarget,
    pub brush: ID2D1SolidColorBrush,
    /// Text formats are expensive to create; cache them per style.
    formats: RefCell<HashMap<FormatKey, IDWriteTextFormat>>,
    icon_family: Vec<u16>,
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

            let icon_family = pick_icon_family(&dwrite);
            Ok(Self {
                factory,
                dwrite,
                target,
                rt,
                brush,
                formats: RefCell::new(HashMap::new()),
                icon_family,
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

    /// Legacy text helper used by the picker layouts: one line, trimmed with
    /// an ellipsis. `center` centres both ways; otherwise text is top-left.
    pub fn draw_text(
        &self,
        text: &str,
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
        font_size: f32,
        bold: bool,
        center: bool,
    ) {
        let weight = if bold { 600 } else { 400 };
        let align = if center { Align::Center } else { Align::Leading };
        let Some(format) = self.format(Font::Text, font_size, weight, align, center) else {
            return;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.brush.SetColor(&color);
            self.rt.DrawText(
                &wide,
                &format,
                &rect,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    fn format(
        &self,
        font: Font,
        size: f32,
        weight: u32,
        align: Align,
        vcenter: bool,
    ) -> Option<IDWriteTextFormat> {
        let key = (font, (size * 100.0) as u32, weight, align, vcenter);
        if let Some(f) = self.formats.borrow().get(&key) {
            return Some(f.clone());
        }
        unsafe {
            let family: Vec<u16> = match font {
                Font::Text => "Segoe UI Variable Text\0".encode_utf16().collect(),
                Font::Display => "Segoe UI Variable Display\0".encode_utf16().collect(),
                Font::Icons => self.icon_family.clone(),
            };
            let format = self
                .dwrite
                .CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )
                .ok()?;
            let _ = format.SetTextAlignment(match align {
                Align::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
            });
            let _ = format.SetParagraphAlignment(if vcenter {
                DWRITE_PARAGRAPH_ALIGNMENT_CENTER
            } else {
                DWRITE_PARAGRAPH_ALIGNMENT_NEAR
            });
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            if let Ok(sign) = self.dwrite.CreateEllipsisTrimmingSign(&format) {
                let trimming = DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                };
                let _ = format.SetTrimming(&trimming, &sign);
            }
            self.formats.borrow_mut().insert(key, format.clone());
            Some(format)
        }
    }

    /// Single-line text, vertically centred in `rect`, trimmed with an
    /// ellipsis when it doesn't fit. `weight` is a CSS-style weight (400, 600).
    pub fn text(
        &self,
        text: &str,
        rect: D2D_RECT_F,
        color: D2D1_COLOR_F,
        size: f32,
        weight: u32,
        font: Font,
        align: Align,
    ) {
        let Some(format) = self.format(font, size, weight, align, true) else {
            return;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.brush.SetColor(&color);
            self.rt.DrawText(
                &wide,
                &format,
                &rect,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    /// Draw an icon glyph centred in `rect`.
    pub fn icon(&self, glyph: char, rect: D2D_RECT_F, color: D2D1_COLOR_F, size: f32) {
        let mut buf = [0u8; 4];
        self.text(
            glyph.encode_utf8(&mut buf),
            rect,
            color,
            size,
            400,
            Font::Icons,
            Align::Center,
        );
    }

    /// Width in DIPs of `text` on one line.
    pub fn measure(&self, text: &str, size: f32, weight: u32, font: Font) -> f32 {
        let Some(format) = self.format(font, size, weight, Align::Leading, true) else {
            return 0.0;
        };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            match self.dwrite.CreateTextLayout(&wide, &format, 10_000.0, 1_000.0) {
                Ok(layout) => {
                    let mut metrics = DWRITE_TEXT_METRICS::default();
                    if layout.GetMetrics(&mut metrics).is_ok() {
                        metrics.widthIncludingTrailingWhitespace
                    } else {
                        0.0
                    }
                }
                Err(_) => 0.0,
            }
        }
    }

    /// Fill a rounded rect with a linear gradient running from `from` to `to`
    /// (absolute coordinates). `stops` are (offset 0..1, colour) pairs.
    pub fn fill_rounded_gradient(
        &self,
        rect: D2D_RECT_F,
        radius: f32,
        from: (f32, f32),
        to: (f32, f32),
        stops: &[(f32, D2D1_COLOR_F)],
    ) {
        let stops: Vec<D2D1_GRADIENT_STOP> = stops
            .iter()
            .map(|&(position, color)| D2D1_GRADIENT_STOP { position, color })
            .collect();
        unsafe {
            let Ok(collection) =
                self.rt
                    .CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
            else {
                return;
            };
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F { x: from.0, y: from.1 },
                endPoint: D2D_POINT_2F { x: to.0, y: to.1 },
            };
            let Ok(brush) = self.rt.CreateLinearGradientBrush(&props, None, &collection) else {
                return;
            };
            let Ok(brush) = brush.cast::<ID2D1Brush>() else {
                return;
            };
            let rounded = D2D1_ROUNDED_RECT {
                rect,
                radiusX: radius,
                radiusY: radius,
            };
            self.rt.FillRoundedRectangle(&rounded, &brush);
        }
    }

    /// Clip subsequent drawing to `rect` until `pop_clip`.
    pub fn push_clip(&self, rect: D2D_RECT_F) {
        unsafe {
            self.rt
                .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        }
    }

    pub fn pop_clip(&self) {
        unsafe {
            self.rt.PopAxisAlignedClip();
        }
    }

    /// Closed polygon (e.g. a hexagon) in absolute coordinates.
    pub fn polygon(&self, points: &[(f32, f32)]) -> Option<ID2D1PathGeometry> {
        if points.len() < 3 {
            return None;
        }
        unsafe {
            let geometry = self.factory.CreatePathGeometry().ok()?;
            let sink = geometry.Open().ok()?;
            sink.BeginFigure(
                D2D_POINT_2F {
                    x: points[0].0,
                    y: points[0].1,
                },
                D2D1_FIGURE_BEGIN_FILLED,
            );
            let rest: Vec<D2D_POINT_2F> = points[1..]
                .iter()
                .map(|&(x, y)| D2D_POINT_2F { x, y })
                .collect();
            sink.AddLines(&rest);
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            sink.Close().ok()?;
            Some(geometry)
        }
    }

    pub fn fill_geometry(&self, geometry: &ID2D1PathGeometry, color: D2D1_COLOR_F) {
        unsafe {
            self.brush.SetColor(&color);
            self.rt.FillGeometry(geometry, &self.brush, None);
        }
    }

    pub fn draw_geometry(&self, geometry: &ID2D1PathGeometry, color: D2D1_COLOR_F, width: f32) {
        unsafe {
            self.brush.SetColor(&color);
            self.rt.DrawGeometry(geometry, &self.brush, width, None);
        }
    }

    /// Brush that paints `bitmap` stretched over `dest` (absolute coords).
    /// Create once per frame and reuse it for many shapes.
    pub fn bitmap_brush(&self, bitmap: &ID2D1Bitmap, dest: D2D_RECT_F, opacity: f32) -> Option<ID2D1Brush> {
        unsafe {
            let size = bitmap.GetSize();
            let w = dest.right - dest.left;
            let h = dest.bottom - dest.top;
            if size.width <= 0.0 || size.height <= 0.0 || w <= 0.0 || h <= 0.0 {
                return None;
            }
            let props = D2D1_BITMAP_BRUSH_PROPERTIES {
                extendModeX: D2D1_EXTEND_MODE_CLAMP,
                extendModeY: D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
            };
            let brush = self.rt.CreateBitmapBrush(bitmap, Some(&props), None).ok()?;
            brush.SetTransform(&windows::Foundation::Numerics::Matrix3x2 {
                M11: w / size.width,
                M12: 0.0,
                M21: 0.0,
                M22: h / size.height,
                M31: dest.left,
                M32: dest.top,
            });
            brush.SetOpacity(opacity);
            brush.cast::<ID2D1Brush>().ok()
        }
    }

    pub fn fill_geometry_with(&self, geometry: &ID2D1PathGeometry, brush: &ID2D1Brush) {
        unsafe {
            self.rt.FillGeometry(geometry, brush, None);
        }
    }

    pub fn draw_ellipse(&self, cx: f32, cy: f32, rx: f32, ry: f32, color: D2D1_COLOR_F, width: f32) {
        unsafe {
            self.brush.SetColor(&color);
            self.rt.DrawEllipse(
                &D2D1_ELLIPSE {
                    point: D2D_POINT_2F { x: cx, y: cy },
                    radiusX: rx,
                    radiusY: ry,
                },
                &self.brush,
                width,
                None,
            );
        }
    }

    /// Aliased rendering makes adjacent shapes tile without hairline seams.
    pub fn set_aliased(&self, aliased: bool) {
        unsafe {
            self.rt.SetAntialiasMode(if aliased {
                D2D1_ANTIALIAS_MODE_ALIASED
            } else {
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE
            });
        }
    }

    pub fn fill_circle(&self, cx: f32, cy: f32, radius: f32, color: D2D1_COLOR_F) {
        self.fill_rounded_rect(
            D2D_RECT_F {
                left: cx - radius,
                top: cy - radius,
                right: cx + radius,
                bottom: cy + radius,
            },
            radius,
            color,
        );
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

/// Segoe Fluent Icons ships with Windows 11; Windows 10 has the older
/// Segoe MDL2 Assets with the same code points for the glyphs we use.
fn pick_icon_family(dwrite: &IDWriteFactory) -> Vec<u16> {
    let fluent = "Segoe Fluent Icons";
    unsafe {
        let mut collection: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut collection, false).is_ok() {
            if let Some(collection) = collection {
                let name: Vec<u16> = format!("{fluent}\0").encode_utf16().collect();
                let mut index = 0u32;
                let mut exists = windows::Win32::Foundation::BOOL(0);
                if collection
                    .FindFamilyName(PCWSTR(name.as_ptr()), &mut index, &mut exists)
                    .is_ok()
                    && exists.as_bool()
                {
                    return name;
                }
            }
        }
    }
    "Segoe MDL2 Assets\0".encode_utf16().collect()
}

/// Corners of a pointy-top hexagon with circumradius `r`, squeezed
/// horizontally by `sx` (1.0 = regular; used for "turning over" effects).
pub fn hexagon(cx: f32, cy: f32, r: f32, sx: f32) -> [(f32, f32); 6] {
    std::array::from_fn(|k| {
        let angle = (60.0 * k as f32 - 90.0).to_radians();
        (cx + r * angle.cos() * sx, cy + r * angle.sin())
    })
}

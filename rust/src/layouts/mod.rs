pub mod dock;
pub mod expose_cloud;
pub mod hero;
pub mod row;
pub mod spotlight;

use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub trait PickerLayout {
    fn update(&mut self, focus: f32, selected: usize);
    fn set_query(&mut self, _query: &str) {}
    fn visible_indices(&self, windows: &[WindowInfo]) -> Vec<usize> {
        (0..windows.len()).collect()
    }
    fn render(
        &self,
        d2d: &D2DContext,
        windows: &[WindowInfo],
        bitmaps: &[Option<ID2D1Bitmap>],
        icons: &[Option<ID2D1Bitmap>],
        accent: D2D1_COLOR_F,
        sw: f32,
        sh: f32,
    );
    fn hit_test(&self, px: f32, py: f32, windows: &[WindowInfo], sw: f32, sh: f32)
        -> Option<usize>;
}

/// Shared Windows 11 (Fluent, dark) styling for every picker layout.
pub mod style {
    use crate::render::d2d::{Align, D2DContext, Font};
    use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
    use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r, g, b, a }
    }

    pub const TEXT1: D2D1_COLOR_F = rgba(0.96, 0.965, 0.975, 1.0);
    pub const TEXT2: D2D1_COLOR_F = rgba(0.76, 0.78, 0.82, 1.0);
    pub const TEXT3: D2D1_COLOR_F = rgba(0.56, 0.58, 0.62, 1.0);
    pub const STROKE: D2D1_COLOR_F = rgba(1.0, 1.0, 1.0, 0.09);
    /// Fill behind thumbnails while they load / letterboxing.
    pub const THUMB_BG: D2D1_COLOR_F = rgba(0.09, 0.095, 0.105, 1.0);

    fn inset(r: D2D_RECT_F, d: f32) -> D2D_RECT_F {
        D2D_RECT_F {
            left: r.left + d,
            top: r.top + d,
            right: r.right - d,
            bottom: r.bottom - d,
        }
    }

    /// Soft two-layer drop shadow under a floating surface.
    pub fn shadow(d2d: &D2DContext, r: D2D_RECT_F, radius: f32) {
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: r.left - 6.0,
                top: r.top + 2.0,
                right: r.right + 6.0,
                bottom: r.bottom + 16.0,
            },
            radius + 8.0,
            rgba(0.0, 0.0, 0.0, 0.10),
        );
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: r.left - 1.0,
                top: r.top + 2.0,
                right: r.right + 1.0,
                bottom: r.bottom + 6.0,
            },
            radius + 2.0,
            rgba(0.0, 0.0, 0.0, 0.22),
        );
    }

    /// Acrylic-like floating surface (flyouts, shelves, panels).
    pub fn surface(d2d: &D2DContext, r: D2D_RECT_F, radius: f32, raised: bool) {
        shadow(d2d, r, radius);
        d2d.fill_rounded_rect(
            r,
            radius,
            if raised {
                rgba(0.165, 0.17, 0.185, 0.90)
            } else {
                rgba(0.125, 0.13, 0.145, 0.82)
            },
        );
        d2d.draw_rounded_rect(r, radius, STROKE, 1.0);
        // Fluent surfaces catch a faint highlight along the top edge.
        d2d.fill_rect(
            D2D_RECT_F {
                left: r.left + radius,
                top: r.top + 1.0,
                right: r.right - radius,
                bottom: r.top + 2.0,
            },
            rgba(1.0, 1.0, 1.0, 0.05),
        );
    }

    /// Selection: a crisp accent ring with a small gap, like Windows 11 focus
    /// visuals, instead of a glowing neon outline.
    pub fn selection(d2d: &D2DContext, r: D2D_RECT_F, radius: f32, accent: D2D1_COLOR_F) {
        d2d.draw_rounded_rect(inset(r, -4.0), radius + 4.0, accent, 2.5);
    }

    /// Hairline frame around an unselected thumbnail.
    pub fn frame(d2d: &D2DContext, r: D2D_RECT_F, radius: f32) {
        d2d.draw_rounded_rect(r, radius, STROKE, 1.0);
    }

    /// App icon on a small rounded tile.
    pub fn icon_tile(d2d: &D2DContext, icon: &ID2D1Bitmap, r: D2D_RECT_F) {
        let size = r.right - r.left;
        let radius = (size * 0.24).clamp(4.0, 9.0);
        d2d.fill_rounded_rect(r, radius, rgba(0.11, 0.115, 0.125, 0.92));
        d2d.draw_rounded_rect(r, radius, rgba(1.0, 1.0, 1.0, 0.10), 1.0);
        let pad = (size * 0.16).clamp(2.5, 6.0);
        d2d.draw_bitmap(icon, inset(r, pad), 1.0);
    }

    /// Single-line label, vertically centred, trimmed with an ellipsis.
    pub fn label(
        d2d: &D2DContext,
        text: &str,
        r: D2D_RECT_F,
        color: D2D1_COLOR_F,
        size: f32,
        weight: u32,
        align: Align,
    ) {
        d2d.text(text, r, color, size, weight, Font::Text, align);
    }

    /// Glass pill sized to `text`, centred horizontally on `cx`.
    pub fn title_pill(
        d2d: &D2DContext,
        text: &str,
        icon: Option<&ID2D1Bitmap>,
        cx: f32,
        top: f32,
        height: f32,
        max_w: f32,
    ) {
        let size = 14.0;
        let icon_w = if icon.is_some() { 26.0 } else { 0.0 };
        let text_w = d2d.measure(text, size, 600, Font::Text);
        let w = (text_w + icon_w + 36.0).clamp(160.0, max_w);
        let r = D2D_RECT_F {
            left: cx - w * 0.5,
            top,
            right: cx + w * 0.5,
            bottom: top + height,
        };
        surface(d2d, r, height * 0.5, true);
        let mut text_left = r.left + 18.0;
        if let Some(icon) = icon {
            let cy = top + height * 0.5;
            d2d.draw_bitmap(
                icon,
                D2D_RECT_F {
                    left: r.left + 14.0,
                    top: cy - 10.0,
                    right: r.left + 34.0,
                    bottom: cy + 10.0,
                },
                1.0,
            );
            text_left = r.left + 14.0 + icon_w;
        }
        label(
            d2d,
            text,
            D2D_RECT_F {
                left: text_left,
                right: r.right - 18.0,
                ..r
            },
            TEXT1,
            size,
            600,
            if icon.is_some() { Align::Leading } else { Align::Center },
        );
    }
}

/// Kept for existing call sites: now draws the shared Fluent surface.
pub fn draw_liquid_glass(
    d2d: &D2DContext,
    rect: D2D_RECT_F,
    radius: f32,
    _accent: D2D1_COLOR_F,
    active: bool,
) {
    style::surface(d2d, rect, radius, active);
}

pub fn draw_contained_thumbnail(
    d2d: &D2DContext,
    rect: D2D_RECT_F,
    bitmap: Option<&ID2D1Bitmap>,
    opacity: f32,
) {
    let Some(bitmap) = bitmap else { return };
    let size = unsafe { bitmap.GetSize() };
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    if size.width <= 0.0 || size.height <= 0.0 || w <= 0.0 || h <= 0.0 {
        return;
    }
    d2d.fill_rect(rect, style::THUMB_BG);
    let scale = (w / size.width).min(h / size.height);
    let dw = size.width * scale;
    let dh = size.height * scale;
    d2d.draw_bitmap(
        bitmap,
        D2D_RECT_F {
            left: rect.left + (w - dw) * 0.5,
            top: rect.top + (h - dh) * 0.5,
            right: rect.left + (w + dw) * 0.5,
            bottom: rect.top + (h + dh) * 0.5,
        },
        opacity,
    );
}

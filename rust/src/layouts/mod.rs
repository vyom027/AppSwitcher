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

/// Shared Windows Direct2D approximation of Liquid Glass: translucent cool
/// material, a soft floating shadow, a tinted active state, and a fine top
/// reflection with restrained double-edge highlights.
pub fn draw_liquid_glass(
    d2d: &D2DContext,
    rect: D2D_RECT_F,
    radius: f32,
    accent: D2D1_COLOR_F,
    active: bool,
) {
    let inset = 1.5;
    d2d.fill_rounded_rect(
        D2D_RECT_F {
            left: rect.left + 2.0,
            top: rect.top + 5.0,
            right: rect.right + 2.0,
            bottom: rect.bottom + 5.0,
        },
        radius + 2.0,
        D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.24,
        },
    );
    d2d.fill_rounded_rect(
        rect,
        radius,
        D2D1_COLOR_F {
            r: 0.68,
            g: 0.77,
            b: 0.93,
            a: if active { 0.20 } else { 0.13 },
        },
    );
    if active {
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: rect.left + 1.0,
                top: rect.top + 1.0,
                right: rect.right - 1.0,
                bottom: rect.bottom - 1.0,
            },
            (radius - 1.0).max(0.0),
            D2D1_COLOR_F {
                r: accent.r,
                g: accent.g,
                b: accent.b,
                a: 0.09,
            },
        );
    }
    d2d.draw_rounded_rect(
        rect,
        radius,
        D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: if active { 0.48 } else { 0.28 },
        },
        if active { 1.5 } else { 1.0 },
    );
    d2d.draw_rounded_rect(
        D2D_RECT_F {
            left: rect.left + inset,
            top: rect.top + inset,
            right: rect.right - inset,
            bottom: rect.bottom - inset,
        },
        (radius - inset).max(0.0),
        D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.07,
        },
        1.0,
    );
    let line_y = rect.top + 1.8;
    d2d.fill_rect(
        D2D_RECT_F {
            left: rect.left + radius.max(8.0),
            top: line_y,
            right: rect.right - radius.max(8.0),
            bottom: line_y + 1.0,
        },
        D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: if active { 0.58 } else { 0.34 },
        },
    );
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

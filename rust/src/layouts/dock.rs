use crate::layouts::{style, PickerLayout};
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct DockLayout {
    pub mag: f32,
    pub card_w: f32,
    pub card_h: f32,
    pub gap: f32,
    focus: f32,
    selected: usize,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            mag: 1.85,
            card_w: 136.0,
            card_h: 86.0,
            gap: 16.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl DockLayout {
    /// Lorentzian magnification curve: sharper, Apple-grade dock focus
    fn magnification(&self, i: usize) -> f32 {
        let d = (i as f32 - self.focus).abs();
        1.0 + (self.mag - 1.0) / (1.0 + d * d * 1.5)
    }
}

impl PickerLayout for DockLayout {
    fn update(&mut self, focus: f32, selected: usize) {
        self.focus = focus;
        self.selected = selected;
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
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }

        // ── 1. Vertical Metric Bands (Maximized Big Theater Preview) ───────────────
        let base_y = sh - 28.0;
        let max_dock_h = self.card_h * self.mag;
        let shelf_top = base_y - max_dock_h - 12.0;
        let title_y = shelf_top - 38.0;
        let preview_bottom = title_y - 14.0;
        let top_bound = 48.0; // cleanly below top pill badge (14..40)

        let available_h = (preview_bottom - top_bound).max(200.0);

        // ── 2. Natural Aspect Ratio of Selected Window (Grand Theater Preview) ─────
        let aspect = if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                unsafe {
                    let sz = bmp.GetSize();
                    if sz.width > 0.0 && sz.height > 0.0 {
                        sz.width / sz.height
                    } else {
                        1.6
                    }
                }
            } else {
                1.6
            }
        } else {
            1.6
        };

        let max_h = available_h;
        let max_w = (sw * 0.90).min(max_h * aspect);
        let mut pw = max_w;
        let mut ph = pw / aspect;
        if ph > max_h {
            ph = max_h;
            pw = ph * aspect;
        }

        let prev_left = (sw - pw) * 0.5;
        let prev_top = top_bound + (available_h - ph) * 0.5;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + pw,
            bottom: prev_top + ph,
        };

        // Preview card: soft shadow, neutral fill, hairline frame.
        style::shadow(d2d, prev_rect, 12.0);
        d2d.fill_rounded_rect(prev_rect, 12.0, style::THUMB_BG);
        if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                d2d.fill_rounded_bitmap(bmp, prev_rect, 12.0, 1.0);
            }
        }
        d2d.draw_rounded_rect(prev_rect, 12.0, style::rgba(1.0, 1.0, 1.0, 0.14), 1.0);

        if let Some(Some(icon_bmp)) = icons.get(self.selected) {
            style::icon_tile(
                d2d,
                icon_bmp,
                D2D_RECT_F {
                    left: prev_left + 14.0,
                    top: prev_top + 14.0,
                    right: prev_left + 50.0,
                    bottom: prev_top + 50.0,
                },
            );
        }

        // ── 3. Active window title ─────────────────────────────────────────
        if let Some(w) = windows.get(self.selected) {
            style::title_pill(d2d, &w.title, None, sw * 0.5, title_y, 34.0, (sw * 0.70).min(650.0));
        }

        // ── 4. Bottom Floating Dock Shelf ────────────────────────────────────
        let mut widths = Vec::with_capacity(n);
        let mut total_w = 0.0;
        for i in 0..n {
            let m = self.magnification(i);
            let w = self.card_w * m;
            widths.push(w);
            total_w += w;
            if i + 1 < n {
                total_w += self.gap;
            }
        }

        let mut x = (sw - total_w) * 0.5;

        // Shelf background pill
        let shelf_padding = 20.0;
        let shelf_rect = D2D_RECT_F {
            left: x - shelf_padding,
            top: shelf_top,
            right: x + total_w + shelf_padding,
            bottom: base_y + 12.0,
        };

        style::surface(d2d, shelf_rect, 18.0, false);

        // ── 5. Render Dock Cards ─────────────────────────────────────────────
        for i in 0..n {
            let m = self.magnification(i);
            let w = widths[i];
            let h = self.card_h * m;
            let y = base_y - h;

            let card_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };

            let is_sel = i == self.selected;

            d2d.fill_rounded_rect(card_rect, 8.0, style::THUMB_BG);
            if let Some(Some(bmp)) = bitmaps.get(i) {
                d2d.fill_rounded_bitmap(bmp, card_rect, 8.0, if is_sel { 1.0 } else { 0.72 });
            }
            if is_sel {
                style::selection(d2d, card_rect, 8.0, accent);
            } else {
                style::frame(d2d, card_rect, 8.0);
            }

            if let Some(Some(icon_bmp)) = icons.get(i) {
                let b = (22.0 * m).clamp(22.0, 34.0);
                style::icon_tile(
                    d2d,
                    icon_bmp,
                    D2D_RECT_F {
                        left: card_rect.left + 8.0,
                        top: card_rect.top + 8.0,
                        right: card_rect.left + 8.0 + b,
                        bottom: card_rect.top + 8.0 + b,
                    },
                );
            }

            x += w + self.gap;
        }
    }

    fn hit_test(&self, px: f32, py: f32, windows: &[WindowInfo], sw: f32, sh: f32) -> Option<usize> {
        let count = windows.len();
        // Must match the baseline used in render().
        let base_y = sh - 28.0;
        let mut widths = Vec::with_capacity(count);
        let mut total_w = 0.0;
        for i in 0..count {
            let m = self.magnification(i);
            let w = self.card_w * m;
            widths.push(w);
            total_w += w;
            if i + 1 < count {
                total_w += self.gap;
            }
        }

        let mut x = (sw - total_w) * 0.5;
        for i in 0..count {
            let m = self.magnification(i);
            let w = widths[i];
            let h = self.card_h * m;
            let y = base_y - h;

            if px >= x && px <= x + w && py >= y && py <= y + h {
                return Some(i);
            }
            x += w + self.gap;
        }

        None
    }
}

use crate::layouts::{style, PickerLayout};
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct HeroLayout {
    pub film_w: f32,
    pub film_h: f32,
    pub gap: f32,
    focus: f32,
    selected: usize,
}

impl Default for HeroLayout {
    fn default() -> Self {
        Self {
            film_w: 140.0,
            film_h: 88.0,
            gap: 16.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for HeroLayout {
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
        let total_film_w = n as f32 * self.film_w + (n as f32 - 1.0) * self.gap;
        let shelf_w = (total_film_w + 32.0).clamp(240.0, sw - 60.0);
        let shelf_h = self.film_h + 20.0;
        let shelf_left = (sw - shelf_w) * 0.5;
        let shelf_top = sh - 18.0 - shelf_h;
        let film_y = shelf_top + 10.0;

        let title_y = shelf_top - 42.0;
        let preview_bottom = title_y - 12.0;
        let top_bound = 48.0;
        let available_h = (preview_bottom - top_bound).max(180.0);

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

        style::shadow(d2d, prev_rect, 12.0);
        d2d.fill_rounded_rect(prev_rect, 12.0, style::THUMB_BG);
        if let Some(Some(bmp)) = bitmaps.get(self.selected) {
            d2d.fill_rounded_bitmap(bmp, prev_rect, 12.0, 1.0);
        }
        d2d.draw_rounded_rect(prev_rect, 12.0, style::rgba(1.0, 1.0, 1.0, 0.14), 1.0);

        // App icon badge in the preview's top-left corner
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

        // Title pill with the app icon
        if let Some(w) = windows.get(self.selected) {
            style::title_pill(
                d2d,
                &w.title,
                icons.get(self.selected).and_then(Option::as_ref),
                sw * 0.5,
                title_y,
                36.0,
                (sw * 0.7).min(640.0),
            );
        }

        // ── 2. Frosted Glass Shelf behind filmstrip ────────────────────────────────
        let shelf_rect = D2D_RECT_F {
            left: shelf_left,
            top: shelf_top,
            right: shelf_left + shelf_w,
            bottom: shelf_top + shelf_h,
        };
        style::surface(d2d, shelf_rect, 16.0, false);

        // 2. Bottom Filmstrip
        let film_stride = self.film_w + self.gap;
        let ox = sw * 0.5 - (self.focus * film_stride) - (self.film_w * 0.5);

        for i in 0..n {
            let x = ox + i as f32 * film_stride;
            let y = film_y;

            if x + self.film_w < 0.0 || x > sw {
                continue;
            }

            let card_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + self.film_w,
                bottom: y + self.film_h,
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
                style::icon_tile(
                    d2d,
                    icon_bmp,
                    D2D_RECT_F {
                        left: x + 6.0,
                        top: y + 6.0,
                        right: x + 31.0,
                        bottom: y + 31.0,
                    },
                );
            }
        }
    }

    fn hit_test(&self, px: f32, py: f32, windows: &[WindowInfo], sw: f32, sh: f32) -> Option<usize> {
        let count = windows.len();
        // Must match render(): shelf_top = sh - 18 - (film_h + 20), film_y = shelf_top + 10.
        let film_y = sh - 28.0 - self.film_h;
        let film_stride = self.film_w + self.gap;
        let ox = sw * 0.5 - (self.focus * film_stride) - (self.film_w * 0.5);

        for i in 0..count {
            let x = ox + i as f32 * film_stride;
            let y = film_y;

            if px >= x && px <= x + self.film_w && py >= y && py <= y + self.film_h {
                return Some(i);
            }
        }
        None
    }
}

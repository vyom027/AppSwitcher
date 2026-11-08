use crate::layouts::PickerLayout;
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
        accent: D2D1_COLOR_F,
        sw: f32,
        sh: f32,
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }

        let film_y = sh - 36.0 - self.film_h;
        let preview_h = (film_y - 80.0).clamp(120.0, sh * 0.58);
        let preview_w = (preview_h * 1.6).min(sw * 0.88);

        // 1. Big Hero Preview
        let prev_left = (sw - preview_w) * 0.5;
        let prev_top = 48.0;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + preview_w,
            bottom: prev_top + preview_h,
        };

        d2d.draw_glow(prev_rect, 18.0, accent, 4);

        d2d.fill_rounded_rect(
            prev_rect,
            18.0,
            D2D1_COLOR_F {
                r: 0.09,
                g: 0.11,
                b: 0.15,
                a: 0.95,
            },
        );

        if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                d2d.draw_bitmap(bmp, prev_rect, 1.0);
            }
        }

        d2d.draw_rounded_rect(prev_rect, 18.0, accent, 2.0);

        // Title
        let title_rect = D2D_RECT_F {
            left: 40.0,
            top: prev_top + preview_h + 12.0,
            right: sw - 40.0,
            bottom: prev_top + preview_h + 46.0,
        };
        let title = if self.selected < windows.len() {
            &windows[self.selected].title
        } else {
            ""
        };
        d2d.draw_text(
            title,
            title_rect,
            D2D1_COLOR_F {
                r: 0.95,
                g: 0.96,
                b: 1.0,
                a: 1.0,
            },
            18.0,
            true,
            true,
        );

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

            if is_sel {
                d2d.draw_glow(card_rect, 12.0, accent, 3);
            }

            d2d.fill_rounded_rect(
                card_rect,
                12.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.75 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.draw_bitmap(bmp, card_rect, if is_sel { 1.0 } else { 0.75 });
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(card_rect, 12.0, accent, 2.5);
            } else {
                d2d.draw_rounded_rect(
                    card_rect,
                    12.0,
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.10,
                    },
                    1.0,
                );
            }
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let film_y = sh - 36.0 - self.film_h;
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

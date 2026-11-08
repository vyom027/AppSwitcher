use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
        thumbnails: &[Option<ImageBuffer>],
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
                r: 0.1,
                g: 0.12,
                b: 0.16,
                a: 0.95,
            },
        );

        if self.selected < thumbnails.len() {
            if let Some(ref thumb) = thumbnails[self.selected] {
                if let Ok(bmp) = d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels) {
                    d2d.draw_bitmap(&bmp, prev_rect, 1.0);
                }
            }
        }

        d2d.draw_rounded_rect(prev_rect, 18.0, accent, 2.5);

        // Window Title
        if self.selected < windows.len() {
            let title = &windows[self.selected].title;
            let title_rect = D2D_RECT_F {
                left: 100.0,
                top: prev_top + preview_h + 8.0,
                right: sw - 100.0,
                bottom: prev_top + preview_h + 34.0,
            };
            d2d.draw_text(
                title,
                title_rect,
                D2D1_COLOR_F {
                    r: 0.95,
                    g: 0.96,
                    b: 0.98,
                    a: 1.0,
                },
                16.0,
                true,
                true,
            );
        }

        // 2. Filmstrip of cards below
        let total_w = n as f32 * self.film_w + (n as f32 - 1.0) * self.gap;
        let mut x = (sw - total_w) * 0.5;

        for i in 0..n {
            let is_sel = i == self.selected;
            let rect = D2D_RECT_F {
                left: x,
                top: film_y,
                right: x + self.film_w,
                bottom: film_y + self.film_h,
            };

            d2d.fill_rounded_rect(
                rect,
                10.0,
                D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.14,
                    b: 0.18,
                    a: if is_sel { 1.0 } else { 0.7 },
                },
            );

            if i < thumbnails.len() {
                if let Some(ref thumb) = thumbnails[i] {
                    if let Ok(bmp) = d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels) {
                        d2d.draw_bitmap(&bmp, rect, if is_sel { 1.0 } else { 0.75 });
                    }
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(rect, 10.0, accent, 2.5);
            }

            x += self.film_w + self.gap;
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let n = count;
        if n == 0 {
            return None;
        }

        let film_y = sh - 36.0 - self.film_h;
        if py < film_y || py > film_y + self.film_h {
            return None;
        }

        let total_w = n as f32 * self.film_w + (n as f32 - 1.0) * self.gap;
        let mut x = (sw - total_w) * 0.5;

        for i in 0..n {
            if px >= x && px <= x + self.film_w {
                return Some(i);
            }
            x += self.film_w + self.gap;
        }

        None
    }
}


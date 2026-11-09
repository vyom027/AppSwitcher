use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct CoverflowLayout {
    pub card_w: f32,
    pub card_h: f32,
    pub step: f32,
    focus: f32,
    selected: usize,
}

impl Default for CoverflowLayout {
    fn default() -> Self {
        Self {
            card_w: 340.0,
            card_h: 212.0,
            step: 180.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for CoverflowLayout {
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

        let cy = sh * 0.5 - 20.0;
        let mut indices: Vec<usize> = (0..n).collect();
        // Sort from edges inward so center card renders last (on top)
        indices.sort_by(|&a, &b| {
            let da = (a as f32 - self.focus).abs();
            let db = (b as f32 - self.focus).abs();
            db.partial_cmp(&da).unwrap()
        });

        for i in indices {
            let offset = i as f32 - self.focus;
            let abs_off = offset.abs();

            let scale = if abs_off < 0.5 {
                1.30 - abs_off * 0.25
            } else {
                (1.0 - (abs_off - 0.5) * 0.12).max(0.68)
            };

            let x_disp = offset.signum() * (abs_off.min(1.0) * self.step + (abs_off - 1.0).max(0.0) * (self.step * 0.72));
            let cx = sw * 0.5 + x_disp;

            let w = self.card_w * scale;
            let h = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - w * 0.5,
                top: cy - h * 0.5,
                right: cx + w * 0.5,
                bottom: cy + h * 0.5,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            // Card background
            d2d.fill_rounded_rect(
                rect,
                16.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.8 },
                },
            );

            // Thumbnail with smooth antialiased 16px corners
            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, rect, 16.0, if is_sel { 1.0 } else { 0.78 });
                }
            }

            // Outline
            if is_sel {
                d2d.draw_rounded_rect(rect, 16.0, accent, 2.5);
            } else {
                d2d.draw_rounded_rect(
                    rect,
                    16.0,
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

        // Active title
        let title_rect = D2D_RECT_F {
            left: 40.0,
            top: cy + self.card_h * 0.75 + 16.0,
            right: sw - 40.0,
            bottom: cy + self.card_h * 0.75 + 56.0,
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
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let cy = sh * 0.5 - 20.0;
        for i in 0..count {
            let offset = i as f32 - self.focus;
            let abs_off = offset.abs();
            let scale = if abs_off < 0.5 {
                1.30 - abs_off * 0.25
            } else {
                (1.0 - (abs_off - 0.5) * 0.12).max(0.68)
            };

            let x_disp = offset.signum() * (abs_off.min(1.0) * self.step + (abs_off - 1.0).max(0.0) * (self.step * 0.72));
            let cx = sw * 0.5 + x_disp;
            let w = self.card_w * scale;
            let h = self.card_h * scale;

            if px >= cx - w * 0.5 && px <= cx + w * 0.5 && py >= cy - h * 0.5 && py <= cy + h * 0.5 {
                return Some(i);
            }
        }
        None
    }
}

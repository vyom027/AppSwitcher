use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct WalletLayout {
    pub card_w: f32,
    pub card_h: f32,
    focus: f32,
    selected: usize,
}

impl Default for WalletLayout {
    fn default() -> Self {
        Self {
            card_w: 480.0,
            card_h: 220.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for WalletLayout {
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

        let cy = sh * 0.5 - 10.0;
        let cx = sw * 0.5;

        // Render back-to-front
        let mut indices: Vec<usize> = (0..n).collect();
        indices.sort_by_key(|&i| -(i as isize - self.selected as isize).abs());

        for i in indices {
            let offset = i as f32 - self.selected as f32;
            let abs_off = offset.abs();

            let scale = if abs_off == 0.0 {
                1.0
            } else {
                (0.94 - (abs_off - 1.0) * 0.06).max(0.8)
            };

            let y_shift = if offset < 0.0 {
                offset * 48.0
            } else {
                offset * 54.0
            };

            let w = self.card_w * scale;
            let h = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - w * 0.5,
                top: cy + y_shift - h * 0.5,
                right: cx + w * 0.5,
                bottom: cy + y_shift + h * 0.5,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                16.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.78 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, rect, 16.0, if is_sel { 1.0 } else { 0.8 });
                }
            }

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

        // Title
        let title_rect = D2D_RECT_F {
            left: 40.0,
            top: cy - self.card_h * 0.5 - 54.0,
            right: sw - 40.0,
            bottom: cy - self.card_h * 0.5 - 18.0,
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
        let cy = sh * 0.5 - 10.0;
        let cx = sw * 0.5;

        for i in 0..count {
            let offset = i as f32 - self.selected as f32;
            let y_shift = if offset < 0.0 {
                offset * 48.0
            } else {
                offset * 54.0
            };

            let w = self.card_w;
            let h = self.card_h;

            if px >= cx - w * 0.5 && px <= cx + w * 0.5 && py >= cy + y_shift - h * 0.5 && py <= cy + y_shift + h * 0.5 {
                return Some(i);
            }
        }
        None
    }
}

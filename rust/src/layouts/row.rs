use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct RowLayout {
    pub card_w: f32,
    pub card_h: f32,
    pub gap: f32,
    focus: f32,
    selected: usize,
}

impl Default for RowLayout {
    fn default() -> Self {
        Self {
            card_w: 240.0,
            card_h: 150.0,
            gap: 28.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for RowLayout {
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

        let total_w = n as f32 * self.card_w + (n as f32 - 1.0) * self.gap;
        let fit = (sw - 120.0) / total_w;
        let scale_factor = fit.min(1.0);

        let cw = self.card_w * scale_factor;
        let ch = self.card_h * scale_factor;
        let gap = self.gap * scale_factor;

        let total_scaled_w = n as f32 * cw + (n as f32 - 1.0) * gap;
        let x0 = (sw - total_scaled_w) * 0.5;
        let cy = sh * 0.5 - 20.0;

        for i in 0..n {
            let offset = (i as f32 - self.focus).abs();
            let pop = 1.0 + 0.14 / (1.0 + offset * offset * 2.0);

            let w = cw * pop;
            let h = ch * pop;

            let bx = x0 + i as f32 * (cw + gap);
            let cx = bx + (cw - w) * 0.5;
            let dy = cy - h * 0.5;

            let rect = D2D_RECT_F {
                left: cx,
                top: dy,
                right: cx + w,
                bottom: dy + h,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(rect, 14.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.8 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, rect, 14.0, if is_sel { 1.0 } else { 0.82 });
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(rect, 14.0, accent, 2.5);
            } else {
                d2d.draw_rounded_rect(
                    rect,
                    14.0,
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.10,
                    },
                    1.0,
                );
            }

            // Title label
            let text_rect = D2D_RECT_F {
                left: cx + 10.0,
                top: dy + h - 32.0,
                right: cx + w - 10.0,
                bottom: dy + h - 6.0,
            };
            d2d.draw_text(
                &windows[i].title,
                text_rect,
                D2D1_COLOR_F {
                    r: 0.95,
                    g: 0.96,
                    b: 1.0,
                    a: if is_sel { 1.0 } else { 0.75 },
                },
                12.0,
                is_sel,
                true,
            );
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let total_w = count as f32 * self.card_w + (count as f32 - 1.0) * self.gap;
        let fit = (sw - 120.0) / total_w;
        let scale_factor = fit.min(1.0);

        let cw = self.card_w * scale_factor;
        let ch = self.card_h * scale_factor;
        let gap = self.gap * scale_factor;

        let total_scaled_w = count as f32 * cw + (count as f32 - 1.0) * gap;
        let x0 = (sw - total_scaled_w) * 0.5;
        let cy = sh * 0.5 - 20.0;

        for i in 0..count {
            let bx = x0 + i as f32 * (cw + gap);
            let dy = cy - ch * 0.5;

            if px >= bx && px <= bx + cw && py >= dy && py <= dy + ch {
                return Some(i);
            }
        }
        None
    }
}

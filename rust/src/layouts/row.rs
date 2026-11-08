use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
        thumbnails: &[Option<ImageBuffer>],
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
            let pop = 1.0 + 0.22 * (-offset * offset / 0.5).exp();
            let w = cw * pop;
            let h = ch * pop;

            let x = x0 + i as f32 * (cw + gap) + (cw - w) * 0.5;
            let y = cy - h * 0.5;

            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
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
                    a: if is_sel { 1.0 } else { 0.82 },
                },
            );

            if i < thumbnails.len() {
                if let Some(ref thumb) = thumbnails[i] {
                    if let Ok(bmp) = d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels) {
                        d2d.draw_bitmap(&bmp, rect, if is_sel { 1.0 } else { 0.85 });
                    }
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(rect, 14.0, accent, 2.5);
            }
        }

        if self.selected < windows.len() {
            let title = &windows[self.selected].title;
            let title_rect = D2D_RECT_F {
                left: 100.0,
                top: cy + ch * 0.5 + 26.0,
                right: sw - 100.0,
                bottom: cy + ch * 0.5 + 54.0,
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
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let n = count;
        if n == 0 {
            return None;
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
            let x = x0 + i as f32 * (cw + gap);
            let y = cy - ch * 0.5;

            if px >= x && px <= x + cw && py >= y && py <= y + ch {
                return Some(i);
            }
        }

        None
    }
}


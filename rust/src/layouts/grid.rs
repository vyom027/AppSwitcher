use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct GridLayout {
    focus: f32,
    selected: usize,
}

impl Default for GridLayout {
    fn default() -> Self {
        Self {
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for GridLayout {
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

        let cols = (n.min(4) as f32).max(1.0);
        let rows = ((n as f32) / cols).ceil();
        let margin = 120.0;
        let gap = 24.0;

        let tw = (sw - 2.0 * margin - (cols - 1.0) * gap) / cols;
        let th = tw * 0.62;

        let total_w = cols * tw + (cols - 1.0) * gap;
        let total_h = rows * th + (rows - 1.0) * gap;

        let ox = (sw - total_w) * 0.5;
        let oy = (sh - total_h) * 0.5 - 20.0;

        for i in 0..n {
            let r = (i as f32 / cols).floor();
            let c = (i as f32) % cols;

            let x = ox + c * (tw + gap);
            let y = oy + r * (th + gap);

            let is_sel = i == self.selected;
            let d = (i as f32 - self.focus).abs();
            let scale = 1.0 + 0.08 / (1.0 + d * d * 3.0);

            let w = tw * scale;
            let h = th * scale;
            let cx = x + (tw - w) * 0.5;
            let cy = y + (th - h) * 0.5;

            let rect = D2D_RECT_F {
                left: cx,
                top: cy,
                right: cx + w,
                bottom: cy + h,
            };

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
                    a: if is_sel { 1.0 } else { 0.82 },
                },
            );

            // Thumbnail
            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.draw_bitmap(bmp, rect, if is_sel { 1.0 } else { 0.85 });
                }
            }

            // Card outline
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

            // Title label
            let text_rect = D2D_RECT_F {
                left: cx + 12.0,
                top: cy + h - 34.0,
                right: cx + w - 12.0,
                bottom: cy + h - 8.0,
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
                13.0,
                is_sel,
                true,
            );
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let cols = (count.min(4) as f32).max(1.0);
        let rows = ((count as f32) / cols).ceil();
        let margin = 120.0;
        let gap = 24.0;

        let tw = (sw - 2.0 * margin - (cols - 1.0) * gap) / cols;
        let th = tw * 0.62;

        let total_w = cols * tw + (cols - 1.0) * gap;
        let total_h = rows * th + (rows - 1.0) * gap;

        let ox = (sw - total_w) * 0.5;
        let oy = (sh - total_h) * 0.5 - 20.0;

        for i in 0..count {
            let r = (i as f32 / cols).floor();
            let c = (i as f32) % cols;

            let x = ox + c * (tw + gap);
            let y = oy + r * (th + gap);

            if px >= x && px <= x + tw && py >= y && py <= y + th {
                return Some(i);
            }
        }
        None
    }
}

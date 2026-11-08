use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
        thumbnails: &[Option<ImageBuffer>],
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
            let scale = if is_sel { 1.05 } else { 1.0 };
            let cw = tw * scale;
            let ch = th * scale;
            let cx = x - (cw - tw) * 0.5;
            let cy = y - (ch - th) * 0.5;

            let rect = D2D_RECT_F {
                left: cx,
                top: cy,
                right: cx + cw,
                bottom: cy + ch,
            };

            if is_sel {
                d2d.draw_glow(rect, 14.0, accent, 3);
            }

            d2d.fill_rounded_rect(
                rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.14,
                    b: 0.18,
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

            // Window Title below each tile
            let text_rect = D2D_RECT_F {
                left: cx - 10.0,
                top: cy + ch + 6.0,
                right: cx + cw + 10.0,
                bottom: cy + ch + 28.0,
            };
            d2d.draw_text(
                &windows[i].title,
                text_rect,
                if is_sel {
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    }
                } else {
                    D2D1_COLOR_F {
                        r: 0.7,
                        g: 0.75,
                        b: 0.82,
                        a: 0.85,
                    }
                },
                13.0,
                is_sel,
                true,
            );
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let n = count;
        if n == 0 {
            return None;
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

            if px >= x && px <= x + tw && py >= y && py <= y + th {
                return Some(i);
            }
        }

        None
    }
}


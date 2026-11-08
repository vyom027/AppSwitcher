use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
        thumbnails: &[Option<ImageBuffer>],
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

            let cw = self.card_w * scale;
            let ch = self.card_h * scale;
            let y = cy + offset * self.card_h * 0.38 - ch * 0.5;
            let x = cx - cw * 0.5;

            let rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + cw,
                bottom: y + ch,
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
                    a: if is_sel { 1.0 } else { (0.9 - abs_off * 0.18).max(0.4) },
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
                d2d.draw_rounded_rect(rect, 16.0, accent, 2.5);
            }
        }

        if self.selected < windows.len() {
            let title = &windows[self.selected].title;
            let title_rect = D2D_RECT_F {
                left: 100.0,
                top: cy + self.card_h * 0.5 + 26.0,
                right: sw - 100.0,
                bottom: cy + self.card_h * 0.5 + 56.0,
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

        let cx = sw * 0.5;
        let cy = sh * 0.5 - 10.0;

        for i in 0..n {
            let offset = i as f32 - self.selected as f32;
            let y = cy + offset * self.card_h * 0.38 - self.card_h * 0.5;
            let x = cx - self.card_w * 0.5;

            if px >= x && px <= x + self.card_w && py >= y && py <= y + self.card_h {
                return Some(i);
            }
        }

        None
    }
}


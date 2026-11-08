use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
            card_w: 320.0,
            card_h: 200.0,
            step: 175.0,
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
        thumbnails: &[Option<ImageBuffer>],
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
                1.0
            } else {
                (0.86 - (abs_off - 0.5) * 0.12).max(0.6)
            };

            let cx = sw * 0.5 + offset * self.step;
            let cw = self.card_w * scale;
            let ch = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - cw * 0.5,
                top: cy - ch * 0.5,
                right: cx + cw * 0.5,
                bottom: cy + ch * 0.5,
            };

            let opacity = (1.0 - abs_off * 0.22).max(0.35);
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
                    a: opacity,
                },
            );

            if i < thumbnails.len() {
                if let Some(ref thumb) = thumbnails[i] {
                    if let Ok(bmp) = d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels) {
                        d2d.draw_bitmap(&bmp, rect, opacity);
                    }
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(rect, 16.0, accent, 2.5);
            }
        }

        // Active Title centered at bottom
        if self.selected < windows.len() {
            let title = &windows[self.selected].title;
            let title_rect = D2D_RECT_F {
                left: 100.0,
                top: cy + self.card_h * 0.5 + 24.0,
                right: sw - 100.0,
                bottom: cy + self.card_h * 0.5 + 54.0,
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

        let cy = sh * 0.5 - 20.0;
        if py < cy - self.card_h * 0.5 || py > cy + self.card_h * 0.5 {
            return None;
        }

        let rel_x = px - sw * 0.5;
        let estimated_idx = (self.focus + rel_x / self.step).round() as isize;

        if estimated_idx >= 0 && (estimated_idx as usize) < n {
            Some(estimated_idx as usize)
        } else {
            None
        }
    }
}


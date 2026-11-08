use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

pub struct FanLayout {
    pub card_w: f32,
    pub card_h: f32,
    pub angle_step: f32,
    focus: f32,
    selected: usize,
}

impl Default for FanLayout {
    fn default() -> Self {
        Self {
            card_w: 240.0,
            card_h: 150.0,
            angle_step: 11.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for FanLayout {
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

        let cy = sh * 0.5 - 30.0;
        let arc_r = sw * 0.9;
        let pivot_y = cy + arc_r;

        let mut indices: Vec<usize> = (0..n).collect();
        indices.sort_by(|&a, &b| {
            let da = (a as f32 - self.focus).abs();
            let db = (b as f32 - self.focus).abs();
            db.partial_cmp(&da).unwrap()
        });

        for i in indices {
            let offset = i as f32 - self.focus;
            let ang_rad = (offset * self.angle_step).to_radians();

            let cx = sw * 0.5 + arc_r * ang_rad.sin();
            let card_cy = pivot_y - arc_r * ang_rad.cos();

            let abs_off = offset.abs();
            let scale = if abs_off < 0.5 {
                1.15
            } else {
                (1.0 - 0.08 * abs_off).max(0.65)
            };

            let cw = self.card_w * scale;
            let ch = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - cw * 0.5,
                top: card_cy - ch * 0.5,
                right: cx + cw * 0.5,
                bottom: card_cy + ch * 0.5,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { (1.0 - abs_off * 0.15).max(0.4) },
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
                d2d.draw_rounded_rect(rect, 14.0, accent, 2.5);
            }
        }

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

        let cy = sh * 0.5 - 30.0;
        let arc_r = sw * 0.9;
        let pivot_y = cy + arc_r;

        for i in 0..n {
            let offset = i as f32 - self.focus;
            let ang_rad = (offset * self.angle_step).to_radians();

            let cx = sw * 0.5 + arc_r * ang_rad.sin();
            let card_cy = pivot_y - arc_r * ang_rad.cos();

            if (px - cx).abs() <= self.card_w * 0.5 && (py - card_cy).abs() <= self.card_h * 0.5 {
                return Some(i);
            }
        }

        None
    }
}


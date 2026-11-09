use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

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
        bitmaps: &[Option<ID2D1Bitmap>],
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
            let dy = pivot_y - arc_r * ang_rad.cos();

            let is_sel = i == self.selected;
            let scale = if is_sel { 1.25 } else { 1.0 };
            let w = self.card_w * scale;
            let h = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - w * 0.5,
                top: dy - h * 0.5,
                right: cx + w * 0.5,
                bottom: dy + h * 0.5,
            };

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
                    a: if is_sel { 1.0 } else { 0.78 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, rect, 14.0, if is_sel { 1.0 } else { 0.8 });
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
        }

        // Title
        let title_rect = D2D_RECT_F {
            left: 40.0,
            top: cy - 140.0,
            right: sw - 40.0,
            bottom: cy - 100.0,
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
        let cy = sh * 0.5 - 30.0;
        let arc_r = sw * 0.9;
        let pivot_y = cy + arc_r;

        for i in 0..count {
            let offset = i as f32 - self.focus;
            let ang_rad = (offset * self.angle_step).to_radians();

            let cx = sw * 0.5 + arc_r * ang_rad.sin();
            let dy = pivot_y - arc_r * ang_rad.cos();

            if px >= cx - self.card_w * 0.5 && px <= cx + self.card_w * 0.5 && py >= dy - self.card_h * 0.5 && py <= dy + self.card_h * 0.5 {
                return Some(i);
            }
        }
        None
    }
}

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
        icons: &[Option<ID2D1Bitmap>],
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

        for &i in &indices {
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
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.09,
                    g: 0.11,
                    b: 0.15,
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

            // Fan card App Icon Badge
            if i < icons.len() {
                if let Some(ref icon_bmp) = icons[i] {
                    let b_size = (22.0 * scale).clamp(20.0, 28.0);
                    let b_pad = 2.5;
                    let b_rect = D2D_RECT_F {
                        left: rect.left + 8.0,
                        top: rect.top + 8.0,
                        right: rect.left + 8.0 + b_size + b_pad * 2.0,
                        bottom: rect.top + 8.0 + b_size + b_pad * 2.0,
                    };
                    d2d.fill_rounded_rect(
                        b_rect,
                        6.0,
                        D2D1_COLOR_F {
                            r: 0.05,
                            g: 0.07,
                            b: 0.10,
                            a: 0.88,
                        },
                    );
                    d2d.draw_rounded_rect(
                        b_rect,
                        6.0,
                        D2D1_COLOR_F {
                            r: 1.0,
                            g: 1.0,
                            b: 1.0,
                            a: 0.20,
                        },
                        1.0,
                    );
                    let ic_rect = D2D_RECT_F {
                        left: b_rect.left + b_pad,
                        top: b_rect.top + b_pad,
                        right: b_rect.right - b_pad,
                        bottom: b_rect.bottom - b_pad,
                    };
                    d2d.draw_bitmap(icon_bmp, ic_rect, 1.0);
                }
            }
        }

        // Active title pill above fan
        if self.selected < windows.len() {
            let active_title = &windows[self.selected].title;
            let title_display = if active_title.chars().count() > 50 {
                format!("{}...", active_title.chars().take(50).collect::<String>())
            } else {
                active_title.clone()
            };
            let pill_h = 36.0;
            let pill_y = (cy - 140.0).max(52.0);
            let pill_w = (title_display.len() as f32 * 8.0 + 70.0).clamp(240.0, 600.0);
            let pill_rect = D2D_RECT_F {
                left: (sw - pill_w) * 0.5,
                top: pill_y,
                right: (sw + pill_w) * 0.5,
                bottom: pill_y + pill_h,
            };
            d2d.fill_rounded_rect(
                pill_rect,
                18.0,
                D2D1_COLOR_F {
                    r: 0.05,
                    g: 0.07,
                    b: 0.10,
                    a: 0.88,
                },
            );
            d2d.draw_rounded_rect(
                pill_rect,
                18.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.18,
                },
                1.0,
            );

            if self.selected < icons.len() {
                if let Some(ref icon_bmp) = icons[self.selected] {
                    let ic_rect = D2D_RECT_F {
                        left: pill_rect.left + 10.0,
                        top: pill_rect.top + 7.0,
                        right: pill_rect.left + 32.0,
                        bottom: pill_rect.top + 29.0,
                    };
                    d2d.draw_bitmap(icon_bmp, ic_rect, 1.0);
                }
            }

            let text_rect = D2D_RECT_F {
                left: pill_rect.left + 38.0,
                top: pill_rect.top,
                right: pill_rect.right - 14.0,
                bottom: pill_rect.bottom,
            };
            d2d.draw_text(
                &title_display,
                text_rect,
                D2D1_COLOR_F {
                    r: 0.96,
                    g: 0.97,
                    b: 1.0,
                    a: 1.0,
                },
                14.0,
                true,
                false,
            );
        }
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

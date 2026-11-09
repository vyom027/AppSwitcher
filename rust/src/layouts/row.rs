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
        icons: &[Option<ID2D1Bitmap>],
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
        let cy = sh * 0.5 - 10.0;

        // Centered Header Badge for Active Window
        if self.selected < windows.len() {
            let active_title = &windows[self.selected].title;
            let title_display = if active_title.chars().count() > 50 {
                format!("{}...", active_title.chars().take(50).collect::<String>())
            } else {
                active_title.clone()
            };
            let pill_h = 36.0;
            let pill_y = cy - ch * 0.5 - 60.0;
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

            // Active Icon in header pill
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
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.09,
                    g: 0.11,
                    b: 0.15,
                    a: if is_sel { 1.0 } else { 0.82 },
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

            // Top-left App Icon Badge
            if i < icons.len() {
                if let Some(ref icon_bmp) = icons[i] {
                    let b_size = 24.0;
                    let b_pad = 3.0;
                    let b_rect = D2D_RECT_F {
                        left: cx + 8.0,
                        top: dy + 8.0,
                        right: cx + 8.0 + b_size + b_pad * 2.0,
                        bottom: dy + 8.0 + b_size + b_pad * 2.0,
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

            // Bottom title banner
            let banner_rect = D2D_RECT_F {
                left: cx,
                top: dy + h - 34.0,
                right: cx + w,
                bottom: dy + h,
            };
            d2d.fill_rounded_rect(
                banner_rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.04,
                    g: 0.06,
                    b: 0.09,
                    a: 0.85,
                },
            );

            let text_rect = D2D_RECT_F {
                left: cx + 10.0,
                top: dy + h - 30.0,
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
                    a: if is_sel { 1.0 } else { 0.78 },
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

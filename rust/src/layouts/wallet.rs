use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

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

        let cy = sh * 0.5 - 10.0;
        let cx = sw * 0.5;

        // Render back-to-front
        let mut indices: Vec<usize> = (0..n).collect();
        indices.sort_by_key(|&i| -(i as isize - self.selected as isize).abs());

        for &i in &indices {
            let offset = i as f32 - self.selected as f32;
            let abs_off = offset.abs();

            let scale = if abs_off == 0.0 {
                1.0
            } else {
                (0.94 - (abs_off - 1.0) * 0.06).max(0.8)
            };

            let y_shift = if offset < 0.0 {
                offset * 48.0
            } else {
                offset * 54.0
            };

            let w = self.card_w * scale;
            let h = self.card_h * scale;

            let rect = D2D_RECT_F {
                left: cx - w * 0.5,
                top: cy + y_shift - h * 0.5,
                right: cx + w * 0.5,
                bottom: cy + y_shift + h * 0.5,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(rect, 16.0, accent, 4);
            }

            d2d.fill_rounded_rect(
                rect,
                16.0,
                D2D1_COLOR_F {
                    r: 0.09,
                    g: 0.11,
                    b: 0.15,
                    a: if is_sel { 1.0 } else { 0.78 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, rect, 16.0, if is_sel { 1.0 } else { 0.8 });
                }
            }

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

            // Wallet card top header badge
            let header_w = (w - 24.0).max(60.0);
            let header_rect = D2D_RECT_F {
                left: rect.left + 12.0,
                top: rect.top + 10.0,
                right: rect.left + 12.0 + header_w,
                bottom: rect.top + 42.0,
            };
            d2d.fill_rounded_rect(
                header_rect,
                8.0,
                D2D1_COLOR_F {
                    r: 0.05,
                    g: 0.07,
                    b: 0.10,
                    a: 0.88,
                },
            );
            d2d.draw_rounded_rect(
                header_rect,
                8.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.18,
                },
                1.0,
            );

            // App Icon
            if i < icons.len() {
                if let Some(ref icon_bmp) = icons[i] {
                    let ic_rect = D2D_RECT_F {
                        left: header_rect.left + 8.0,
                        top: header_rect.top + 5.0,
                        right: header_rect.left + 30.0,
                        bottom: header_rect.top + 27.0,
                    };
                    d2d.draw_bitmap(icon_bmp, ic_rect, 1.0);
                }
            }

            // Title inside card header
            let title_text = &windows[i].title;
            let title_display = if title_text.chars().count() > 36 {
                format!("{}...", title_text.chars().take(36).collect::<String>())
            } else {
                title_text.clone()
            };
            let text_rect = D2D_RECT_F {
                left: header_rect.left + 36.0,
                top: header_rect.top,
                right: header_rect.right - 8.0,
                bottom: header_rect.bottom,
            };
            d2d.draw_text(
                &title_display,
                text_rect,
                D2D1_COLOR_F {
                    r: 0.95,
                    g: 0.96,
                    b: 1.0,
                    a: if is_sel { 1.0 } else { 0.80 },
                },
                12.5,
                is_sel,
                false,
            );
        }

        // Active title pill above the wallet stack
        if self.selected < windows.len() {
            let active_title = &windows[self.selected].title;
            let title_display = if active_title.chars().count() > 50 {
                format!("{}...", active_title.chars().take(50).collect::<String>())
            } else {
                active_title.clone()
            };
            let pill_h = 36.0;
            let pill_y = (cy - self.card_h * 0.5 - 64.0).max(52.0);
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
        let cy = sh * 0.5 - 10.0;
        let cx = sw * 0.5;

        for i in 0..count {
            let offset = i as f32 - self.selected as f32;
            let y_shift = if offset < 0.0 {
                offset * 48.0
            } else {
                offset * 54.0
            };

            let w = self.card_w;
            let h = self.card_h;

            if px >= cx - w * 0.5 && px <= cx + w * 0.5 && py >= cy + y_shift - h * 0.5 && py <= cy + y_shift + h * 0.5 {
                return Some(i);
            }
        }
        None
    }
}

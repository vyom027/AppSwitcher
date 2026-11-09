use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct HeroLayout {
    pub film_w: f32,
    pub film_h: f32,
    pub gap: f32,
    focus: f32,
    selected: usize,
}

impl Default for HeroLayout {
    fn default() -> Self {
        Self {
            film_w: 140.0,
            film_h: 88.0,
            gap: 16.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl PickerLayout for HeroLayout {
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

        let film_y = sh - 36.0 - self.film_h;
        let preview_h = (film_y - 90.0).clamp(120.0, sh * 0.58);
        let preview_w = (preview_h * 1.6).min(sw * 0.88);

        // 1. Big Hero Preview
        let prev_left = (sw - preview_w) * 0.5;
        let prev_top = 48.0;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + preview_w,
            bottom: prev_top + preview_h,
        };

        d2d.draw_glow(prev_rect, 18.0, accent, 4);

        d2d.fill_rounded_rect(
            prev_rect,
            18.0,
            D2D1_COLOR_F {
                r: 0.09,
                g: 0.11,
                b: 0.15,
                a: 0.95,
            },
        );

        if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                d2d.fill_rounded_bitmap(bmp, prev_rect, 18.0, 1.0);
            }
        }

        d2d.draw_rounded_rect(prev_rect, 18.0, accent, 2.0);

        // Hero App Icon Badge in preview top-left
        if self.selected < icons.len() {
            if let Some(ref icon_bmp) = icons[self.selected] {
                let b_rect = D2D_RECT_F {
                    left: prev_left + 16.0,
                    top: prev_top + 16.0,
                    right: prev_left + 54.0,
                    bottom: prev_top + 54.0,
                };
                d2d.fill_rounded_rect(
                    b_rect,
                    10.0,
                    D2D1_COLOR_F {
                        r: 0.04,
                        g: 0.05,
                        b: 0.08,
                        a: 0.88,
                    },
                );
                d2d.draw_rounded_rect(
                    b_rect,
                    10.0,
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.22,
                    },
                    1.0,
                );
                let ic_rect = D2D_RECT_F {
                    left: b_rect.left + 5.0,
                    top: b_rect.top + 5.0,
                    right: b_rect.right - 5.0,
                    bottom: b_rect.bottom - 5.0,
                };
                d2d.draw_bitmap(icon_bmp, ic_rect, 1.0);
            }
        }

        // Title Pill
        if self.selected < windows.len() {
            let active_title = &windows[self.selected].title;
            let title_display = if active_title.chars().count() > 50 {
                format!("{}...", active_title.chars().take(50).collect::<String>())
            } else {
                active_title.clone()
            };
            let pill_h = 36.0;
            let pill_y = prev_top + preview_h + 12.0;
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

        // 2. Bottom Filmstrip
        let film_stride = self.film_w + self.gap;
        let ox = sw * 0.5 - (self.focus * film_stride) - (self.film_w * 0.5);

        for i in 0..n {
            let x = ox + i as f32 * film_stride;
            let y = film_y;

            if x + self.film_w < 0.0 || x > sw {
                continue;
            }

            let card_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + self.film_w,
                bottom: y + self.film_h,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(card_rect, 12.0, accent, 3);
            }

            d2d.fill_rounded_rect(
                card_rect,
                12.0,
                D2D1_COLOR_F {
                    r: 0.1,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.75 },
                },
            );

            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, card_rect, 12.0, if is_sel { 1.0 } else { 0.75 });
                }
            }

            if is_sel {
                d2d.draw_rounded_rect(card_rect, 12.0, accent, 2.5);
            } else {
                d2d.draw_rounded_rect(
                    card_rect,
                    12.0,
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.10,
                    },
                    1.0,
                );
            }

            // Filmstrip Card App Icon Badge
            if i < icons.len() {
                if let Some(ref icon_bmp) = icons[i] {
                    let b_size = 20.0;
                    let b_pad = 2.5;
                    let b_rect = D2D_RECT_F {
                        left: x + 6.0,
                        top: y + 6.0,
                        right: x + 6.0 + b_size + b_pad * 2.0,
                        bottom: y + 6.0 + b_size + b_pad * 2.0,
                    };
                    d2d.fill_rounded_rect(
                        b_rect,
                        5.0,
                        D2D1_COLOR_F {
                            r: 0.05,
                            g: 0.07,
                            b: 0.10,
                            a: 0.88,
                        },
                    );
                    d2d.draw_rounded_rect(
                        b_rect,
                        5.0,
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
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let film_y = sh - 36.0 - self.film_h;
        let film_stride = self.film_w + self.gap;
        let ox = sw * 0.5 - (self.focus * film_stride) - (self.film_w * 0.5);

        for i in 0..count {
            let x = ox + i as f32 * film_stride;
            let y = film_y;

            if px >= x && px <= x + self.film_w && py >= y && py <= y + self.film_h {
                return Some(i);
            }
        }
        None
    }
}

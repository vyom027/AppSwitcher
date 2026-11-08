use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct DockLayout {
    pub mag: f32,
    pub card_w: f32,
    pub card_h: f32,
    pub gap: f32,
    focus: f32,
    selected: usize,
}

impl Default for DockLayout {
    fn default() -> Self {
        Self {
            mag: 1.85,
            card_w: 140.0,
            card_h: 88.0,
            gap: 16.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl DockLayout {
    /// Lorentzian magnification curve: sharper, Apple-grade dock focus
    fn magnification(&self, i: usize) -> f32 {
        let d = (i as f32 - self.focus).abs();
        1.0 + (self.mag - 1.0) / (1.0 + d * d * 1.5)
    }
}

impl PickerLayout for DockLayout {
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

        let base_y = sh - 36.0;
        let max_dock_h = self.card_h * self.mag;
        let preview_bottom = base_y - max_dock_h - 48.0;
        let preview_h = (preview_bottom - 90.0).clamp(160.0, sh * 0.52);
        let preview_w = (preview_h * 1.6).min(sw * 0.80);

        // 1. Render Big Selected Preview Card
        let prev_left = (sw - preview_w) * 0.5;
        let prev_top = preview_bottom - preview_h;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + preview_w,
            bottom: prev_top + preview_h,
        };

        // Ambient colored glow behind the preview card
        d2d.draw_glow(prev_rect, 18.0, accent, 4);

        // Preview card background
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
                d2d.draw_bitmap(bmp, prev_rect, 1.0);
            }
        }

        // Preview card accent outline
        d2d.draw_rounded_rect(prev_rect, 18.0, accent, 2.0);

        // Preview bottom reflection (Subtle Apple-style mirror reflection)
        let refl_h = preview_h * 0.25;
        let refl_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top + preview_h + 4.0,
            right: prev_left + preview_w,
            bottom: prev_top + preview_h + 4.0 + refl_h,
        };
        if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                d2d.draw_bitmap(bmp, refl_rect, 0.18);
            }
        }

        // 2. Active Window Title (Centered above the preview)
        let title_rect = D2D_RECT_F {
            left: 40.0,
            top: prev_top - 46.0,
            right: sw - 40.0,
            bottom: prev_top - 12.0,
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
                r: 0.96,
                g: 0.97,
                b: 1.0,
                a: 1.0,
            },
            19.0,
            true,
            true,
        );

        // 3. Render Bottom Floating Dock Shelf
        let mut widths = Vec::with_capacity(n);
        let mut total_w = 0.0;
        for i in 0..n {
            let m = self.magnification(i);
            let w = self.card_w * m;
            widths.push(w);
            total_w += w;
            if i + 1 < n {
                total_w += self.gap;
            }
        }

        let mut x = (sw - total_w) * 0.5;

        // Shelf background pill
        let shelf_padding = 24.0;
        let shelf_rect = D2D_RECT_F {
            left: x - shelf_padding,
            top: base_y - max_dock_h - 16.0,
            right: x + total_w + shelf_padding,
            bottom: base_y + 14.0,
        };

        // Dark glass shelf fill
        d2d.fill_rounded_rect(
            shelf_rect,
            24.0,
            D2D1_COLOR_F {
                r: 0.07,
                g: 0.09,
                b: 0.13,
                a: 0.72,
            },
        );
        // Subtle frosted border
        d2d.draw_rounded_rect(
            shelf_rect,
            24.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.12,
            },
            1.5,
        );

        // 4. Render Dock Cards
        for i in 0..n {
            let m = self.magnification(i);
            let w = widths[i];
            let h = self.card_h * m;
            let y = base_y - h;

            let card_rect = D2D_RECT_F {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            };

            let is_sel = i == self.selected;

            if is_sel {
                d2d.draw_glow(card_rect, 14.0, accent, 3);
            }

            // Card background
            d2d.fill_rounded_rect(
                card_rect,
                14.0,
                D2D1_COLOR_F {
                    r: 0.10,
                    g: 0.12,
                    b: 0.16,
                    a: if is_sel { 1.0 } else { 0.85 },
                },
            );

            // Thumbnail
            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.draw_bitmap(bmp, card_rect, if is_sel { 1.0 } else { 0.85 });
                }
            }

            // Card outline
            if is_sel {
                d2d.draw_rounded_rect(card_rect, 14.0, accent, 2.5);
            } else {
                d2d.draw_rounded_rect(
                    card_rect,
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

            x += w + self.gap;
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let base_y = sh - 36.0;
        let mut widths = Vec::with_capacity(count);
        let mut total_w = 0.0;
        for i in 0..count {
            let m = self.magnification(i);
            let w = self.card_w * m;
            widths.push(w);
            total_w += w;
            if i + 1 < count {
                total_w += self.gap;
            }
        }

        let mut x = (sw - total_w) * 0.5;
        for i in 0..count {
            let m = self.magnification(i);
            let w = widths[i];
            let h = self.card_h * m;
            let y = base_y - h;

            if px >= x && px <= x + w && py >= y && py <= y + h {
                return Some(i);
            }
            x += w + self.gap;
        }

        None
    }
}

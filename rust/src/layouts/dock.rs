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
            card_w: 136.0,
            card_h: 86.0,
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
        icons: &[Option<ID2D1Bitmap>],
        accent: D2D1_COLOR_F,
        sw: f32,
        sh: f32,
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }

        // ── 1. Vertical Metric Bands (Maximized Big Theater Preview) ───────────────
        let base_y = sh - 28.0;
        let max_dock_h = self.card_h * self.mag;
        let shelf_top = base_y - max_dock_h - 12.0;
        let title_y = shelf_top - 38.0;
        let preview_bottom = title_y - 14.0;
        let top_bound = 48.0; // cleanly below top pill badge (14..40)

        let available_h = (preview_bottom - top_bound).max(200.0);

        // ── 2. Natural Aspect Ratio of Selected Window (Grand Theater Preview) ─────
        let aspect = if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                unsafe {
                    let sz = bmp.GetSize();
                    if sz.width > 0.0 && sz.height > 0.0 {
                        sz.width / sz.height
                    } else {
                        1.6
                    }
                }
            } else {
                1.6
            }
        } else {
            1.6
        };

        let max_h = available_h;
        let max_w = (sw * 0.90).min(max_h * aspect);
        let mut pw = max_w;
        let mut ph = pw / aspect;
        if ph > max_h {
            ph = max_h;
            pw = ph * aspect;
        }

        let prev_left = (sw - pw) * 0.5;
        let prev_top = top_bound + (available_h - ph) * 0.5;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + pw,
            bottom: prev_top + ph,
        };

        // Ambient colored glow behind the preview card
        d2d.draw_glow(prev_rect, 20.0, accent, 5);

        // Preview card dark glass background
        d2d.fill_rounded_rect(
            prev_rect,
            20.0,
            D2D1_COLOR_F {
                r: 0.08,
                g: 0.10,
                b: 0.14,
                a: 0.96,
            },
        );

        // Preview thumbnail with true antialiased 20px rounded corners
        if self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                d2d.fill_rounded_bitmap(bmp, prev_rect, 20.0, 1.0);
            }
        }

        // Preview card accent outline
        d2d.draw_rounded_rect(prev_rect, 20.0, accent, 2.5);

        // Preview card app icon badge
        if self.selected < icons.len() {
            if let Some(ref icon_bmp) = icons[self.selected] {
                let badge_rect = D2D_RECT_F {
                    left: prev_left + 16.0,
                    top: prev_top + 16.0,
                    right: prev_left + 54.0,
                    bottom: prev_top + 54.0,
                };
                d2d.fill_rounded_rect(
                    badge_rect,
                    10.0,
                    D2D1_COLOR_F {
                        r: 0.04,
                        g: 0.05,
                        b: 0.08,
                        a: 0.88,
                    },
                );
                d2d.draw_rounded_rect(
                    badge_rect,
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
                    left: badge_rect.left + 5.0,
                    top: badge_rect.top + 5.0,
                    right: badge_rect.right - 5.0,
                    bottom: badge_rect.bottom - 5.0,
                };
                d2d.draw_bitmap(icon_bmp, ic_rect, 1.0);
            }
        }

        // Subtle Apple-style mirror reflection strictly confined above title
        let refl_top = prev_top + ph + 4.0;
        let max_refl_h = (title_y - refl_top - 4.0).max(0.0);
        let refl_h = (ph * 0.18).min(max_refl_h).min(45.0);
        if refl_h > 4.0 && self.selected < bitmaps.len() {
            if let Some(ref bmp) = bitmaps[self.selected] {
                let refl_rect = D2D_RECT_F {
                    left: prev_left,
                    top: refl_top,
                    right: prev_left + pw,
                    bottom: refl_top + refl_h,
                };
                d2d.fill_rounded_bitmap(bmp, refl_rect, 10.0, 0.16);
            }
        }

        // ── 3. Active Window Title (Refined Liquid Glass Capsule centered above dock) ───────────
        let title = if self.selected < windows.len() {
            let t = &windows[self.selected].title;
            if t.chars().count() > 56 {
                format!("{}...", t.chars().take(56).collect::<String>())
            } else {
                t.clone()
            }
        } else {
            String::new()
        };
        let title_w = (title.len() as f32 * 9.5 + 48.0).clamp(240.0, (sw * 0.70).min(650.0));
        let title_rect = D2D_RECT_F {
            left: (sw - title_w) * 0.5,
            top: title_y,
            right: (sw + title_w) * 0.5,
            bottom: title_y + 34.0,
        };
        d2d.fill_rounded_rect(
            title_rect,
            17.0,
            D2D1_COLOR_F {
                r: 0.05,
                g: 0.07,
                b: 0.10,
                a: 0.84,
            },
        );
        d2d.draw_rounded_rect(
            title_rect,
            17.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.14,
            },
            1.0,
        );
        d2d.draw_text(
            &title,
            title_rect,
            D2D1_COLOR_F {
                r: 0.96,
                g: 0.97,
                b: 1.0,
                a: 1.0,
            },
            14.5,
            true,
            true,
        );

        // ── 4. Bottom Floating Dock Shelf ────────────────────────────────────
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
        let shelf_padding = 20.0;
        let shelf_rect = D2D_RECT_F {
            left: x - shelf_padding,
            top: shelf_top,
            right: x + total_w + shelf_padding,
            bottom: base_y + 12.0,
        };

        // Dark glass shelf fill
        d2d.fill_rounded_rect(
            shelf_rect,
            22.0,
            D2D1_COLOR_F {
                r: 0.06,
                g: 0.08,
                b: 0.11,
                a: 0.78,
            },
        );
        // Subtle frosted border
        d2d.draw_rounded_rect(
            shelf_rect,
            22.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.14,
            },
            1.5,
        );

        // ── 5. Render Dock Cards ─────────────────────────────────────────────
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
                    r: 0.09,
                    g: 0.11,
                    b: 0.15,
                    a: if is_sel { 1.0 } else { 0.80 },
                },
            );

            // Thumbnail with antialiased 14px rounded corners
            if i < bitmaps.len() {
                if let Some(ref bmp) = bitmaps[i] {
                    d2d.fill_rounded_bitmap(bmp, card_rect, 14.0, if is_sel { 1.0 } else { 0.80 });
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

            // App icon badge
            if i < icons.len() {
                if let Some(ref icon_bmp) = icons[i] {
                    let b_size = (20.0 * m).clamp(20.0, 32.0);
                    let b_pad = 3.0;
                    let b_rect = D2D_RECT_F {
                        left: card_rect.left + 8.0,
                        top: card_rect.top + 8.0,
                        right: card_rect.left + 8.0 + b_size + b_pad * 2.0,
                        bottom: card_rect.top + 8.0 + b_size + b_pad * 2.0,
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

            x += w + self.gap;
        }
    }

    fn hit_test(&self, px: f32, py: f32, windows: &[WindowInfo], sw: f32, sh: f32) -> Option<usize> {
        let count = windows.len();
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

use crate::layouts::PickerLayout;
use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

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
            mag: 1.95,
            card_w: 150.0,
            card_h: 94.0,
            gap: 20.0,
            focus: 0.0,
            selected: 0,
        }
    }
}

impl DockLayout {
    pub fn new(mag: f32) -> Self {
        Self {
            mag,
            ..Default::default()
        }
    }

    /// Lorentzian magnification curve: sharper, more Apple-like focus
    fn magnification(&self, i: usize) -> f32 {
        let d = (i as f32 - self.focus).abs();
        1.0 + (self.mag - 1.0) / (1.0 + d * d * 1.4)
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
        thumbnails: &[Option<ImageBuffer>],
        accent: D2D1_COLOR_F,
        sw: f32,
        sh: f32,
    ) {
        let n = windows.len();
        if n == 0 {
            return;
        }

        let base_y = sh - 40.0;
        let max_h = self.card_h * self.mag;
        let preview_bottom = base_y - max_h - 40.0;
        let preview_h = (preview_bottom - 60.0).clamp(100.0, sh * 0.55);
        let preview_w = (preview_h * 1.6).min(sw * 0.85);

        // 1. Render Big Selected Preview Card
        let prev_left = (sw - preview_w) * 0.5;
        let prev_top = preview_bottom - preview_h;
        let prev_rect = D2D_RECT_F {
            left: prev_left,
            top: prev_top,
            right: prev_left + preview_w,
            bottom: prev_top + preview_h,
        };

        // Glow behind preview
        d2d.draw_glow(prev_rect, 18.0, accent, 4);

        // Preview card background
        d2d.fill_rounded_rect(
            prev_rect,
            18.0,
            D2D1_COLOR_F {
                r: 0.1,
                g: 0.12,
                b: 0.16,
                a: 0.95,
            },
        );

        if self.selected < thumbnails.len() {
            if let Some(ref thumb) = thumbnails[self.selected] {
                if let Ok(bmp) = d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels) {
                    d2d.draw_bitmap(&bmp, prev_rect, 1.0);
                }
            }
        }

        // Preview border
        d2d.draw_rounded_rect(prev_rect, 18.0, accent, 2.5);

        // Window Title below preview
        if self.selected < windows.len() {
            let title = &windows[self.selected].title;
            let title_rect = D2D_RECT_F {
                left: prev_left - 50.0,
                top: prev_top + preview_h + 8.0,
                right: prev_left + preview_w + 50.0,
                bottom: prev_top + preview_h + 36.0,
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

        // 2. Render Dock Tray Cards
        let widths: Vec<f32> = (0..n).map(|i| self.card_w * self.magnification(i)).collect();
        let total_w: f32 = widths.iter().sum::<f32>() + (n as f32 - 1.0) * self.gap;
        let mut x = (sw - total_w) * 0.5;

        // Subtle glass shelf behind dock
        let shelf_rect = D2D_RECT_F {
            left: x - 24.0,
            top: base_y - self.card_h * 1.15,
            right: x + total_w + 24.0,
            bottom: base_y + 12.0,
        };
        d2d.fill_rounded_rect(
            shelf_rect,
            24.0,
            D2D1_COLOR_F {
                r: 0.08,
                g: 0.09,
                b: 0.12,
                a: 0.65,
            },
        );
        d2d.draw_rounded_rect(
            shelf_rect,
            24.0,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 0.08,
            },
            1.0,
        );

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

            // Card background
            d2d.fill_rounded_rect(
                card_rect,
                12.0,
                D2D1_COLOR_F {
                    r: 0.12,
                    g: 0.14,
                    b: 0.18,
                    a: if is_sel { 1.0 } else { 0.8 },
                },
            );

            // Thumbnail
            if i < thumbnails.len() {
                if let Some(ref thumb) = thumbnails[i] {
                    if let Ok(bmp) =
                        d2d.create_bitmap_from_rgba(thumb.width, thumb.height, &thumb.pixels)
                    {
                        d2d.draw_bitmap(&bmp, card_rect, if is_sel { 1.0 } else { 0.82 });
                    }
                }
            }

            // Selection ring
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
                        a: 0.08,
                    },
                    1.0,
                );
            }

            x += w + self.gap;
        }
    }

    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize> {
        let n = count;
        if n == 0 {
            return None;
        }

        let base_y = sh - 40.0;
        let widths: Vec<f32> = (0..n).map(|i| self.card_w * self.magnification(i)).collect();
        let total_w: f32 = widths.iter().sum::<f32>() + (n as f32 - 1.0) * self.gap;
        let mut x = (sw - total_w) * 0.5;

        for i in 0..n {
            let w = widths[i];
            let h = self.card_h * self.magnification(i);
            let y = base_y - h;

            if px >= x && px <= x + w && py >= y && py <= base_y {
                return Some(i);
            }
            x += w + self.gap;
        }

        None
    }
}


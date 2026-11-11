use crate::layouts::{draw_contained_thumbnail, draw_liquid_glass, PickerLayout};
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

/// Flat panorama: one large active frame, with neighboring windows arranged
/// edge-to-edge like a film strip. There is no depth or perspective here.
pub struct RowLayout {
    focus: f32,
    selected: usize,
}
impl Default for RowLayout {
    fn default() -> Self {
        Self {
            focus: 0.0,
            selected: 0,
        }
    }
}
impl RowLayout {
    fn geometry(&self, i: usize, sw: f32, sh: f32) -> D2D_RECT_F {
        let delta = i as f32 - self.focus;
        let active = i == self.selected;
        let w = if active { sw * 0.48 } else { sw * 0.30 };
        let h = if active { sh * 0.52 } else { sh * 0.39 };
        let step = sw * 0.385;
        let cx = sw * 0.5 + delta * step;
        let cy = sh * 0.48 + if active { 0.0 } else { sh * 0.035 };
        D2D_RECT_F {
            left: cx - w * 0.5,
            top: cy - h * 0.5,
            right: cx + w * 0.5,
            bottom: cy + h * 0.5,
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
        // The thin baseline makes this read as a continuous panorama, not a grid.
        d2d.fill_rect(
            D2D_RECT_F {
                left: sw * 0.06,
                top: sh * 0.79,
                right: sw * 0.94,
                bottom: sh * 0.792,
            },
            D2D1_COLOR_F {
                r: 0.75,
                g: 0.82,
                b: 0.94,
                a: 0.22,
            },
        );
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| {
            ((*b as f32 - self.focus).abs()).total_cmp(&((*a as f32 - self.focus).abs()))
        });
        for i in order {
            let r = self.geometry(i, sw, sh);
            if r.right < 0.0 || r.left > sw {
                continue;
            }
            let selected = i == self.selected;
            let radius = if selected { 6.0 } else { 3.0 };
            d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: r.left + 5.0,
                    top: r.top + 8.0,
                    right: r.right + 5.0,
                    bottom: r.bottom + 8.0,
                },
                radius,
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.55,
                },
            );
            draw_liquid_glass(d2d, r, radius, accent, selected);
            draw_contained_thumbnail(
                d2d,
                D2D_RECT_F {
                    left: r.left + 3.0,
                    top: r.top + 3.0,
                    right: r.right - 3.0,
                    bottom: r.bottom - 3.0,
                },
                bitmaps.get(i).and_then(Option::as_ref),
                if selected { 1.0 } else { 0.82 },
            );
            if selected {
                d2d.draw_rounded_rect(r, radius, accent, 3.0);
                let title = &windows[i].title;
                d2d.draw_text(
                    title,
                    D2D_RECT_F {
                        left: r.left + 18.0,
                        top: r.bottom + 12.0,
                        right: r.right - 18.0,
                        bottom: r.bottom + 42.0,
                    },
                    D2D1_COLOR_F {
                        r: 0.96,
                        g: 0.97,
                        b: 1.0,
                        a: 1.0,
                    },
                    17.0,
                    true,
                    true,
                );
                if let Some(Some(icon)) = icons.get(i) {
                    d2d.draw_bitmap(
                        icon,
                        D2D_RECT_F {
                            left: r.left + 16.0,
                            top: r.top + 16.0,
                            right: r.left + 56.0,
                            bottom: r.top + 56.0,
                        },
                        1.0,
                    );
                }
            } else {
                d2d.draw_rounded_rect(
                    r,
                    radius,
                    D2D1_COLOR_F {
                        r: 0.72,
                        g: 0.80,
                        b: 0.92,
                        a: 0.25,
                    },
                    1.0,
                );
                d2d.draw_text(
                    &windows[i].title,
                    D2D_RECT_F {
                        left: r.left + 8.0,
                        top: r.bottom + 10.0,
                        right: r.right - 8.0,
                        bottom: r.bottom + 32.0,
                    },
                    D2D1_COLOR_F {
                        r: 0.76,
                        g: 0.82,
                        b: 0.92,
                        a: 0.82,
                    },
                    11.0,
                    false,
                    true,
                );
            }
        }
        d2d.draw_text(
            "PANORAMA",
            D2D_RECT_F {
                left: 26.0,
                top: 18.0,
                right: 260.0,
                bottom: 42.0,
            },
            accent,
            12.0,
            true,
            false,
        );
    }
    fn hit_test(
        &self,
        px: f32,
        py: f32,
        windows: &[WindowInfo],
        sw: f32,
        sh: f32,
    ) -> Option<usize> {
        let n = windows.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| {
            ((*a as f32 - self.focus).abs()).total_cmp(&((*b as f32 - self.focus).abs()))
        });
        order.into_iter().find(|&i| {
            let r = self.geometry(i, sw, sh);
            px >= r.left && px <= r.right && py >= r.top && py <= r.bottom
        })
    }
}

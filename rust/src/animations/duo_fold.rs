use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};

pub struct DuoFoldTransition {
    pub direction: i32,
    pub eye_depth: f32,
    pub max_bend_current: f32,
    pub max_bend_next: f32,
}

impl Default for DuoFoldTransition {
    fn default() -> Self {
        Self {
            direction: 1,
            eye_depth: 1.45,
            max_bend_current: 78.0f32.to_radians(),
            max_bend_next: 24.0f32.to_radians(),
        }
    }
}

impl DuoFoldTransition {
    pub fn new(direction: i32) -> Self {
        Self {
            direction,
            ..Default::default()
        }
    }

    /// Render Duo Fold transition at normalized time t in [0.0, 1.0]
    pub fn render(
        &self,
        d2d: &D2DContext,
        curr_bitmap: &ID2D1Bitmap,
        next_bitmap: &ID2D1Bitmap,
        t: f32,
        sw: f32,
        sh: f32,
    ) {
        let t = t.clamp(0.0, 1.0);
        let curr_hinge_left = self.direction > 0;
        let next_hinge_left = self.direction <= 0;

        // 1. Deep modern obsidian void backdrop behind folding panels (Prevents any white bleed)
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: sw,
                bottom: sh,
            },
            0.0,
            D2D1_COLOR_F {
                r: 0.03,
                g: 0.04,
                b: 0.06,
                a: 0.96,
            },
        );

        // 2. Next panel unfolds forward from 3D depth
        let next_bend = (1.0 - t) * self.max_bend_next;
        let next_scale = 1.0 - next_bend.sin() * 0.12;
        let next_w = sw * next_scale;
        let next_h = sh * next_scale;
        let next_x = if next_hinge_left { 0.0 } else { sw - next_w };
        let next_y = (sh - next_h) * 0.5;

        let next_rect = D2D_RECT_F {
            left: next_x,
            top: next_y,
            right: next_x + next_w,
            bottom: next_y + next_h,
        };

        // Ambient depth shadow under unfolding panel
        if t < 0.95 {
            d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: next_x - 10.0,
                    top: next_y - 8.0,
                    right: next_x + next_w + 10.0,
                    bottom: next_y + next_h + 10.0,
                },
                16.0,
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.40 * (1.0 - t),
                },
            );
        }

        let next_opacity = 0.55 + 0.45 * t;
        d2d.draw_bitmap(next_bitmap, next_rect, next_opacity);

        // 3. Current panel folds away into void
        let curr_bend = t * self.max_bend_current;
        let curr_scale = 1.0 - curr_bend.sin() * 0.28;
        let curr_w = sw * curr_scale;
        let curr_h = sh * curr_scale;
        let curr_x = if curr_hinge_left { 0.0 } else { sw - curr_w };
        let curr_y = (sh - curr_h) * 0.5;

        let curr_rect = D2D_RECT_F {
            left: curr_x,
            top: curr_y,
            right: curr_x + curr_w,
            bottom: curr_y + curr_h,
        };

        let curr_opacity = if t > 0.55 {
            (1.0 - (t - 0.55) / 0.45).max(0.0)
        } else {
            1.0
        };

        if curr_opacity > 0.001 {
            // Shadow behind folding panel
            d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: curr_x - 12.0,
                    top: curr_y - 10.0,
                    right: curr_x + curr_w + 12.0,
                    bottom: curr_y + curr_h + 12.0,
                },
                18.0,
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.55 * curr_opacity,
                },
            );
            d2d.draw_bitmap(curr_bitmap, curr_rect, curr_opacity);
            // Subtle glass edge highlight along folding edge
            d2d.draw_rounded_rect(
                curr_rect,
                4.0,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.12 * curr_opacity,
                },
                1.5,
            );
        }
    }
}


use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

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

    /// Render 3D Perspective Projective Duo Fold transition
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

        // 1. Deep modern obsidian void backdrop behind folding panels
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: sw,
                bottom: sh,
            },
            0.0,
            D2D1_COLOR_F {
                r: 0.02,
                g: 0.03,
                b: 0.05,
                a: 1.0,
            },
        );

        let aspect = sw / sh.max(1.0);
        let eye = self.eye_depth * aspect.max(1.0);

        // 2. Next panel unfolds forward around its hinge
        let bend_n = (1.0 - t) * self.max_bend_next;
        let op_n = 0.50 + 0.50 * t;
        self.render_3d_panel(
            d2d,
            next_bitmap,
            next_hinge_left,
            bend_n,
            op_n,
            eye,
            aspect,
            sw,
            sh,
        );

        // 3. Current panel folds away around its hinge into void
        let bend_c = t * self.max_bend_current;
        let op_c = if t > 0.55 {
            (1.0 - (t - 0.55) / 0.45).max(0.0)
        } else {
            1.0
        };

        if op_c > 0.001 {
            self.render_3d_panel(
                d2d,
                curr_bitmap,
                curr_hinge_left,
                bend_c,
                op_c,
                eye,
                aspect,
                sw,
                sh,
            );
        }
    }

    fn render_3d_panel(
        &self,
        d2d: &D2DContext,
        bitmap: &ID2D1Bitmap,
        hinge_left: bool,
        bend: f32,
        opacity: f32,
        eye: f32,
        aspect: f32,
        sw: f32,
        sh: f32,
    ) {
        let sz = unsafe { bitmap.GetSize() };
        if sz.width <= 0.0 || sz.height <= 0.0 {
            return;
        }

        let cosine = bend.cos();
        let sine = bend.sin();

        let hinge_x = if hinge_left { 0.0 } else { sw };
        let far_x = if hinge_left { sw } else { 0.0 };
        let cy = sh * 0.5;

        let strips = 48;
        let inv_strips = 1.0 / strips as f32;

        for i in 0..strips {
            let u0 = i as f32 * inv_strips;
            let u1 = (i + 1) as f32 * inv_strips;

            // Distance from hinge in [0, 1]
            let fh0 = if hinge_left { u0 } else { 1.0 - u0 };
            let fh1 = if hinge_left { u1 } else { 1.0 - u1 };

            let d0 = 1.35 * aspect * sine * fh0;
            let d1 = 1.35 * aspect * sine * fh1;

            let p0 = eye / (eye + d0);
            let p1 = eye / (eye + d1);

            let x0 = hinge_x + (far_x - hinge_x) * fh0 * cosine * p0;
            let x1 = hinge_x + (far_x - hinge_x) * fh1 * cosine * p1;

            let h0 = cy * p0;
            let h1 = cy * p1;

            let left = x0.min(x1);
            let right = x0.max(x1).max(left + 1.0);
            let top = cy - h0.max(h1);
            let bottom = cy + h0.max(h1);

            let dst_rect = D2D_RECT_F {
                left,
                top,
                right,
                bottom,
            };
            let src_rect = D2D_RECT_F {
                left: u0 * sz.width,
                top: 0.0,
                right: u1 * sz.width,
                bottom: sz.height,
            };

            // Depth-dependent darkening into the void
            let depth_darken = 1.0 - 0.35 * fh0 * sine;
            let strip_opacity = (opacity * depth_darken).clamp(0.0, 1.0);

            d2d.draw_bitmap_subrect(bitmap, dst_rect, src_rect, strip_opacity);
        }
    }
}

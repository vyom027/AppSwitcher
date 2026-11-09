use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct DuoFoldTransition {
    pub direction: i32,
    pub max_bend_current: f32,
    pub max_bend_next: f32,
}

impl Default for DuoFoldTransition {
    fn default() -> Self {
        Self {
            direction: 1,
            max_bend_current: 78.0f32.to_radians(),
            max_bend_next: 26.0f32.to_radians(),
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

        let eye_distance = 1.45 * sw.max(sh);

        // 2. Next panel unfolds forward around its hinge from the deep void
        let bend_n = (1.0 - t) * self.max_bend_next;
        let op_n = 0.50 + 0.50 * t;
        self.render_3d_panel(
            d2d,
            next_bitmap,
            next_hinge_left,
            bend_n,
            op_n,
            eye_distance,
            sw,
            sh,
        );

        // 3. Current panel folds away around its hinge into the void
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
                eye_distance,
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
        let cy = sh * 0.5;

        let strips = 96;
        let inv_strips = 1.0 / strips as f32;

        for i in 0..strips {
            let u0 = i as f32 * inv_strips;
            let u1 = (i + 1) as f32 * inv_strips;

            // Normalized distance from hinge [0, 1]
            let fh0 = if hinge_left { u0 } else { 1.0 - u0 };
            let fh1 = if hinge_left { u1 } else { 1.0 - u1 };
            let fh_avg = (fh0 + fh1) * 0.5;

            // True projective perspective depth
            let depth_z0 = fh0 * sw * sine;
            let depth_z1 = fh1 * sw * sine;

            let p0 = eye / (eye + depth_z0);
            let p1 = eye / (eye + depth_z1);

            let x_disp0 = fh0 * sw * cosine * p0;
            let x_disp1 = fh1 * sw * cosine * p1;

            let x0 = if hinge_left {
                hinge_x + x_disp0
            } else {
                hinge_x - x_disp0
            };
            let x1 = if hinge_left {
                hinge_x + x_disp1
            } else {
                hinge_x - x_disp1
            };

            let h0 = cy * p0;
            let h1 = cy * p1;
            let h_avg = (h0 + h1) * 0.5;

            let left = x0.min(x1);
            let right = x0.max(x1) + 0.6; // Slight subpixel overlap prevents seam
            let top = cy - h_avg;
            let bottom = cy + h_avg;

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

            // 1. Draw projected bitmap strip
            d2d.draw_bitmap_subrect(bitmap, dst_rect, src_rect, opacity);

            // 2. Liquid Glass Deep Void Occlusion Shading
            let dark_fade = ((fh_avg - 0.22) / 0.78).clamp(0.0, 1.0);
            let dark_alpha = (0.90 * dark_fade.powf(1.4) * sine * opacity).clamp(0.0, 1.0);
            if dark_alpha > 0.005 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.02,
                        g: 0.03,
                        b: 0.05,
                        a: dark_alpha,
                    },
                );
            }

            // 3. Liquid Glass Gaussian Specular Reflection Glint
            let refl = (-((fh_avg - 0.68) / 0.18).powi(2)).exp();
            let glass_alpha = (0.30 * refl * (1.0 - bend / 1.5) * opacity).clamp(0.0, 1.0);
            if glass_alpha > 0.005 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.92,
                        g: 0.96,
                        b: 1.0,
                        a: glass_alpha,
                    },
                );
            }

            // 4. Subtle Hinge Edge Drop Shadow
            let hinge_shadow = ((1.0 - fh_avg / 0.22).clamp(0.0, 1.0) * sine * 0.35 * opacity).clamp(0.0, 1.0);
            if hinge_shadow > 0.005 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: hinge_shadow,
                    },
                );
            }
        }
    }
}

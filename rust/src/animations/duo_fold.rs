use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct DuoFoldTransition {
    pub direction: i32,
}

impl Default for DuoFoldTransition {
    fn default() -> Self {
        Self { direction: 1 }
    }
}

impl DuoFoldTransition {
    pub fn new(direction: i32) -> Self {
        Self { direction }
    }

    /// True projective 3D perspective Duo Fold transition (Apple iPhone Duo / Solo style)
    /// Hinge is pinned to the screen edge. Current panel folds away into 3D depth,
    /// while next panel unfolds forward from depth to 100% full screen.
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
        let next_hinge_left = !curr_hinge_left;

        // 1. Deep void backdrop underneath the folding surfaces
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
                b: 0.04,
                a: 1.0,
            },
        );

        // 2. Next window unfolds forward from depth (24 deg -> 0 deg)
        let max_bend_n = 24.0f32.to_radians();
        let bend_n = (1.0 - t) * max_bend_n;
        let opacity_n = 0.60 + 0.40 * t;
        render_projective_fold_panel(
            d2d,
            next_bitmap,
            next_hinge_left,
            bend_n,
            opacity_n,
            sw,
            sh,
            false,
        );

        // Ambient shadow on next panel cast by the folding front panel
        let cast_shadow = (0.45 * (1.0 - t)).clamp(0.0, 1.0);
        if cast_shadow > 0.01 {
            d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                },
                0.0,
                D2D1_COLOR_F {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: cast_shadow,
                },
            );
        }

        // 3. Current window folds away around its hinge into the void (0 deg -> 78 deg)
        let max_bend_c = 78.0f32.to_radians();
        let bend_c = t * max_bend_c;
        let opacity_c = if t > 0.60 {
            (1.0 - (t - 0.60) / 0.40).max(0.0)
        } else {
            1.0
        };
        if opacity_c > 0.01 {
            render_projective_fold_panel(
                d2d,
                curr_bitmap,
                curr_hinge_left,
                bend_c,
                opacity_c,
                sw,
                sh,
                true,
            );
        }
    }
}

/// Render a 3D perspective folding panel pinned to a screen edge hinge using fine vertical projective strips
fn render_projective_fold_panel(
    d2d: &D2DContext,
    bitmap: &ID2D1Bitmap,
    hinge_left: bool,
    bend_rad: f32,
    opacity: f32,
    sw: f32,
    sh: f32,
    is_current: bool,
) {
    let sz = unsafe { bitmap.GetSize() };
    if sz.width <= 0.0 || sz.height <= 0.0 {
        return;
    }

    let cosine = bend_rad.cos();
    let sine = bend_rad.sin();
    let cy = sh * 0.5;
    let hinge_x = if hinge_left { 0.0 } else { sw };

    // 1.45 * sw gives Apple-grade perspective convergence
    let eye = 1.45 * sw;
    let strips = 64;
    let inv_strips = 1.0 / strips as f32;

    for i in 0..strips {
        let u0 = i as f32 * inv_strips;
        let u1 = (i + 1) as f32 * inv_strips;

        // Normalized distance from hinge in original surface: 0.0 at hinge, 1.0 at moving edge
        let from_h0 = if hinge_left { u0 } else { 1.0 - u0 };
        let from_h1 = if hinge_left { u1 } else { 1.0 - u1 };

        let d0 = from_h0 * sw;
        let d1 = from_h1 * sw;

        // 3D position after rotation around hinge
        let x_off0 = d0 * cosine;
        let z0 = d0 * sine;

        let x_off1 = d1 * cosine;
        let z1 = d1 * sine;

        // Perspective scale factor
        let p0 = eye / (eye + z0);
        let p1 = eye / (eye + z1);

        let sx0 = if hinge_left {
            hinge_x + x_off0 * p0
        } else {
            hinge_x - x_off0 * p0
        };
        let sx1 = if hinge_left {
            hinge_x + x_off1 * p1
        } else {
            hinge_x - x_off1 * p1
        };

        let left = sx0.min(sx1);
        let right = sx0.max(sx1) + 0.6; // Subpixel seam coverage

        let top0 = cy - cy * p0;
        let bot0 = cy + cy * p0;
        let top1 = cy - cy * p1;
        let bot1 = cy + cy * p1;

        let top = top0.min(top1);
        let bottom = bot0.max(bot1);

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

        // 1. Draw projected strip
        d2d.draw_bitmap_subrect(bitmap, dst_rect, src_rect, opacity);

        // 2. Liquid Glass depth shading (progressive darkening with depth)
        let mid_h = (from_h0 + from_h1) * 0.5;
        if is_current && bend_rad > 0.02 {
            // Distance-dependent darkening toward pitch black void
            let dark_fade = ((mid_h - 0.20) / 0.80).clamp(0.0, 1.0);
            let dark_alpha = (0.75 * dark_fade * (bend_rad / 1.36)).clamp(0.0, 0.85);
            if dark_alpha > 0.01 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.02,
                        g: 0.03,
                        b: 0.05,
                        a: dark_alpha * opacity,
                    },
                );
            }

            // Specular glass reflection peak at ~70% from hinge
            let spec_dist = (mid_h - 0.70) / 0.22;
            let spec = (-spec_dist * spec_dist).exp();
            let spec_alpha = (0.28 * spec * (1.0 - (bend_rad - 0.5).abs())).clamp(0.0, 0.35);
            if spec_alpha > 0.02 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.92,
                        g: 0.96,
                        b: 1.0,
                        a: spec_alpha * opacity,
                    },
                );
            }
        }
    }
}

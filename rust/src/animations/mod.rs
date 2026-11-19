pub mod duo_fold;
pub mod hex_bloom;
pub mod orbit_throw;
pub mod slide;
pub mod windowed;

pub use duo_fold::DuoFoldTransition;
pub use slide::SlideTransition;

use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub enum TransitionKind {
    Slide,
    DuoFold,
    Fade,
    Zoom,
    Cube,
    Flip,
    Push,
    Stack,
    OrbitThrow,
    HexBloom,
}

impl TransitionKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().replace('_', " ").as_str() {
            "duo fold" | "duofold" => Self::DuoFold,
            "fade" => Self::Fade,
            "zoom" => Self::Zoom,
            "cube" => Self::Cube,
            "flip" => Self::Flip,
            "push" => Self::Push,
            "stack" => Self::Stack,
            "orbit throw" | "orbitthrow" => Self::OrbitThrow,
            "hex bloom" | "hexbloom" => Self::HexBloom,
            _ => Self::Slide,
        }
    }

    /// Multiplier on the user's animation duration. Orbit Throw and Hex
    /// Bloom have far more motion than a slide, so they need longer to read.
    pub fn duration_scale(&self) -> f32 {
        match self {
            Self::OrbitThrow => 2.6,
            Self::HexBloom => 2.4,
            _ => 1.0,
        }
    }

    pub fn render(
        &self,
        d2d: &D2DContext,
        curr: &ID2D1Bitmap,
        next: &ID2D1Bitmap,
        t: f32,
        direction: i32,
        sw: f32,
        sh: f32,
    ) {
        match self {
            Self::Slide => SlideTransition::new(direction).render(d2d, curr, next, t, sw, sh),
            Self::DuoFold => DuoFoldTransition::new(direction).render(d2d, curr, next, t, sw, sh),
            Self::OrbitThrow => orbit_throw::render(d2d, curr, next, t, direction, sw, sh),
            Self::HexBloom => hex_bloom::render(d2d, curr, next, t, direction, sw, sh),
            Self::Fade => {
                // Keep both frames fixed in place: this is a true dissolve,
                // with no directional motion or scale that could read as Zoom.
                let rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                };
                // A small drift in the swipe direction keeps it tied to
                // your fingers without turning it into a slide.
                let drift = direction.signum() as f32 * sw * 0.035;
                let shift = |r: D2D_RECT_F, dx: f32| D2D_RECT_F {
                    left: r.left + dx,
                    right: r.right + dx,
                    ..r
                };
                d2d.draw_bitmap(curr, shift(rect, drift * t), (1.0 - t).max(0.0));
                d2d.draw_bitmap(next, shift(rect, -drift * (1.0 - t)), t.min(1.0));
            }
            Self::Zoom => {
                // Move through the outgoing frame into a tighter incoming frame.
                // The brief overlap keeps the transition continuous while the
                // scale change makes this clearly different from a dissolve.
                // The incoming frame grows out of the swipe side.
                let dir = direction.signum() as f32;
                let curr_s = 1.0 + 0.34 * t;
                let next_s = 0.72 + 0.28 * t;
                let curr_dx = dir * sw * 0.10 * t;
                let next_dx = -dir * sw * 0.18 * (1.0 - t);
                let c_rect = D2D_RECT_F {
                    left: (sw - sw * curr_s) * 0.5 + curr_dx,
                    top: (sh - sh * curr_s) * 0.5,
                    right: (sw + sw * curr_s) * 0.5 + curr_dx,
                    bottom: (sh + sh * curr_s) * 0.5,
                };
                let n_rect = D2D_RECT_F {
                    left: (sw - sw * next_s) * 0.5 + next_dx,
                    top: (sh - sh * next_s) * 0.5,
                    right: (sw + sw * next_s) * 0.5 + next_dx,
                    bottom: (sh + sh * next_s) * 0.5,
                };
                let curr_alpha = (1.0 - (t / 0.72)).clamp(0.0, 1.0);
                let next_alpha = ((t - 0.18) / 0.82).clamp(0.0, 1.0);
                d2d.draw_bitmap(curr, c_rect, curr_alpha);
                d2d.draw_bitmap(next, n_rect, next_alpha);
            }
            Self::Push => {
                // Layered shove: the outgoing frame recedes while the incoming
                // frame travels over it. This keeps Push visually distinct from
                // Slide, where both frames remain flat and move as one pair.
                let dir = direction as f32;
                let curr_s = 1.0 - 0.18 * t;
                let curr_w = sw * curr_s;
                let curr_h = sh * curr_s;
                let curr_x = (sw - curr_w) * 0.5 + dir * t * sw * 0.16;
                let curr_y = (sh - curr_h) * 0.5;
                let curr_rect = D2D_RECT_F {
                    left: curr_x,
                    top: curr_y,
                    right: curr_x + curr_w,
                    bottom: curr_y + curr_h,
                };
                d2d.draw_bitmap(curr, curr_rect, 1.0 - 0.28 * t);

                let next_s = 0.88 + 0.12 * t;
                let next_w = sw * next_s;
                let next_h = sh * next_s;
                let next_x = -dir * (1.0 - t) * sw + (sw - next_w) * 0.5;
                let next_y = (sh - next_h) * 0.5;
                let next_rect = D2D_RECT_F {
                    left: next_x,
                    top: next_y,
                    right: next_x + next_w,
                    bottom: next_y + next_h,
                };

                // Moving edge shadow reinforces the physical push.
                let shadow_x = if dir > 0.0 {
                    next_x + next_w
                } else {
                    next_x - 38.0
                };
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: shadow_x,
                        top: next_y,
                        right: shadow_x + 38.0,
                        bottom: next_y + next_h,
                    },
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.68 * (1.0 - t),
                    },
                );

                d2d.draw_bitmap(next, next_rect, (0.82 + 0.18 * t).min(1.0));
            }
            Self::Stack => {
                // The old frame recedes and dims while the new frame rises
                // vertically, giving this a layered-card feel unlike Push.
                let curr_s = 1.0 - 0.08 * t;
                let curr_w = sw * curr_s;
                let curr_h = sh * curr_s;
                let curr_rect = D2D_RECT_F {
                    left: (sw - curr_w) * 0.5,
                    top: (sh - curr_h) * 0.5,
                    right: (sw + curr_w) * 0.5,
                    bottom: (sh + curr_h) * 0.5,
                };
                d2d.draw_bitmap(curr, curr_rect, 1.0 - 0.35 * t);

                // Next app rises up from below, arriving from the swipe side.
                let next_y = (1.0 - t) * sh;
                let next_x = -(direction.signum() as f32) * sw * 0.3 * (1.0 - t);
                let next_rect = D2D_RECT_F {
                    left: next_x,
                    top: next_y,
                    right: next_x + sw,
                    bottom: next_y + sh,
                };

                // Ambient shadow above rising card
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: next_x,
                        top: next_y - 24.0,
                        right: next_x + sw,
                        bottom: next_y,
                    },
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.55 * (1.0 - t),
                    },
                );

                d2d.draw_bitmap(next, next_rect, 1.0);
            }
            Self::Cube => {
                let dir = direction as f32;
                let angle_curr = -dir * t * 90.0f32.to_radians();
                let angle_next = dir * (1.0 - t) * 90.0f32.to_radians();
                render_3d_rot_panel(d2d, curr, angle_curr, (1.0 - t * 0.4).max(0.0), sw, sh);
                render_3d_rot_panel(d2d, next, angle_next, (0.6 + t * 0.4).min(1.0), sw, sh);
            }
            Self::Flip => {
                // True 3D perspective rotation around center Y-axis with physical foreshortening
                let dir = direction as f32;
                if t < 0.5 {
                    let sub_t = t / 0.5;
                    let angle = -dir * sub_t * 90.0f32.to_radians();
                    render_3d_rot_panel(d2d, curr, angle, 1.0, sw, sh);
                } else {
                    let sub_t = (t - 0.5) / 0.5;
                    let angle = dir * (1.0 - sub_t) * 90.0f32.to_radians();
                    render_3d_rot_panel(d2d, next, angle, 1.0, sw, sh);
                }
            }
        }
    }
}

/// Authentic 3D perspective rotation around center Y-axis using fine vertical strips
fn render_3d_rot_panel(
    d2d: &D2DContext,
    bitmap: &ID2D1Bitmap,
    angle_rad: f32,
    opacity: f32,
    sw: f32,
    sh: f32,
) {
    let sz = unsafe { bitmap.GetSize() };
    if sz.width <= 0.0 || sz.height <= 0.0 {
        return;
    }

    let cosine = angle_rad.cos();
    let sine = angle_rad.sin();
    let _cy = sh * 0.5;
    let cx = sw * 0.5;

    let eye = 1.6 * sw;
    let strips = 64;
    let inv_strips = 1.0 / strips as f32;

    for i in 0..strips {
        let u0 = i as f32 * inv_strips;
        let u1 = (i + 1) as f32 * inv_strips;

        // X relative to center in original panel coordinates [-sw/2, sw/2]
        let rx0 = (u0 - 0.5) * sw;
        let rx1 = (u1 - 0.5) * sw;

        // 3D coordinates after Y-axis rotation
        let x3d_0 = rx0 * cosine;
        let z3d_0 = rx0 * sine;

        let x3d_1 = rx1 * cosine;
        let z3d_1 = rx1 * sine;

        // Perspective foreshortening
        let p0 = eye / (eye + z3d_0);
        let p1 = eye / (eye + z3d_1);

        let sx0 = cx + x3d_0 * p0;
        let sx1 = cx + x3d_1 * p1;

        let left = sx0.min(sx1);
        let right = sx0.max(sx1) + 0.6; // slight subpixel overlap to prevent seams
        let top = 0.0;
        let bottom = sh;

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

        // 2. Liquid Glass 3D Depth Shading: darkening for surface facing away
        let z_norm = ((z3d_0 + z3d_1) * 0.5 / (sw * 0.5)).clamp(-1.0, 1.0);
        let depth_dark = if z_norm > 0.0 { z_norm * 0.65 } else { 0.0 };
        if depth_dark > 0.01 {
            d2d.fill_rounded_rect(
                dst_rect,
                0.0,
                D2D1_COLOR_F {
                    r: 0.02,
                    g: 0.03,
                    b: 0.05,
                    a: (depth_dark * opacity).clamp(0.0, 1.0),
                },
            );
        }

        // 3. Specular highlight when edge sweeps through center
        let ang_norm = angle_rad.abs().to_degrees();
        if ang_norm < 45.0 {
            let spec = (1.0 - (u0 - 0.5).abs() * 2.0).powi(2) * (1.0 - ang_norm / 45.0);
            if spec > 0.02 {
                d2d.fill_rounded_rect(
                    dst_rect,
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.95,
                        g: 0.98,
                        b: 1.0,
                        a: (spec * 0.22 * opacity).clamp(0.0, 1.0),
                    },
                );
            }
        }
    }
}

/// False when the user turned off "Animation effects" in Windows settings
/// (SPI_GETCLIENTAREAANIMATION); transitions and picker motion then snap.
pub fn system_animations_enabled() -> bool {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    let mut enabled = BOOL(1);
    unsafe {
        if SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_err()
        {
            return true;
        }
    }
    enabled.as_bool()
}

static RESPECT_SYSTEM_ANIMATIONS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Mirror the `respect_system_animations` setting (startup and on save).
pub fn set_respect_system_animations(respect: bool) {
    RESPECT_SYSTEM_ANIMATIONS.store(respect, std::sync::atomic::Ordering::Relaxed);
}

/// Whether transitions and picker motion should play. Always true unless the
/// user opted in to following the Windows "Animation effects" setting.
pub fn motion_allowed() -> bool {
    !RESPECT_SYSTEM_ANIMATIONS.load(std::sync::atomic::Ordering::Relaxed)
        || system_animations_enabled()
}

//! Transitions for switching to a window that isn't maximized.
//!
//! The current screen stays in place as the background (the old window really
//! does stay where it is), and only the next window animates, at its real
//! size, into its real position. The last frame therefore matches exactly
//! what Windows shows once the window is brought to the front.
//!
//! `src` is the window's area inside the next bitmap; `dest` is where it sits
//! on screen. `direction` follows the swipe: +1 brings the window in from the
//! left, -1 from the right (the same as Slide).

use super::TransitionKind;
use crate::render::d2d::D2DContext;
use std::f32::consts::PI;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

fn black(a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a,
    }
}

fn width(r: D2D_RECT_F) -> f32 {
    r.right - r.left
}

fn height(r: D2D_RECT_F) -> f32 {
    r.bottom - r.top
}

/// `r` scaled by `s` around its centre, then moved by (dx, dy).
fn transform(r: D2D_RECT_F, s: f32, dx: f32, dy: f32) -> D2D_RECT_F {
    let cx = (r.left + r.right) * 0.5 + dx;
    let cy = (r.top + r.bottom) * 0.5 + dy;
    let hw = width(r) * 0.5 * s;
    let hh = height(r) * 0.5 * s;
    D2D_RECT_F {
        left: cx - hw,
        top: cy - hh,
        right: cx + hw,
        bottom: cy + hh,
    }
}

/// Soft drop shadow under a floating window.
pub fn shadow(d2d: &D2DContext, r: D2D_RECT_F, strength: f32) {
    if strength <= 0.01 || width(r) < 2.0 {
        return;
    }
    d2d.fill_rounded_rect(
        D2D_RECT_F {
            left: r.left - 10.0,
            top: r.top - 4.0,
            right: r.right + 10.0,
            bottom: r.bottom + 22.0,
        },
        18.0,
        black(0.14 * strength),
    );
    d2d.fill_rounded_rect(
        D2D_RECT_F {
            left: r.left - 2.0,
            top: r.top + 2.0,
            right: r.right + 2.0,
            bottom: r.bottom + 8.0,
        },
        10.0,
        black(0.28 * strength),
    );
}

/// Draw the part `src` of `next` into `dest`.
fn window(d2d: &D2DContext, next: &ID2D1Bitmap, src: D2D_RECT_F, dest: D2D_RECT_F, opacity: f32) {
    if width(dest) >= 1.0 && height(dest) >= 1.0 && opacity > 0.005 {
        d2d.draw_bitmap_subrect(next, dest, src, opacity.min(1.0));
    }
}

/// Draw horizontal slice [u0, u1] (0..1 of the window's width) of the window
/// into `dest`.
fn window_slice(
    d2d: &D2DContext,
    next: &ID2D1Bitmap,
    src: D2D_RECT_F,
    u0: f32,
    u1: f32,
    dest: D2D_RECT_F,
    opacity: f32,
) {
    let sw = width(src);
    let part = D2D_RECT_F {
        left: src.left + sw * u0,
        right: src.left + sw * u1,
        ..src
    };
    window(d2d, next, part, dest, opacity);
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    kind: &TransitionKind,
    d2d: &D2DContext,
    curr: &ID2D1Bitmap,
    next: &ID2D1Bitmap,
    src: D2D_RECT_F,
    dest: D2D_RECT_F,
    t: f32,
    direction: i32,
    sw: f32,
    sh: f32,
) {
    let t = t.clamp(0.0, 1.0);
    let dir = if direction >= 0 { 1.0 } else { -1.0 };
    let screen = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: sw,
        bottom: sh,
    };

    match kind {
        TransitionKind::OrbitThrow => {
            return super::orbit_throw::render_windowed(d2d, curr, next, src, dest, t, direction, sw, sh)
        }
        TransitionKind::HexBloom => {
            return super::hex_bloom::render_windowed(d2d, curr, next, src, dest, t, direction, sw, sh)
        }
        _ => {}
    }

    // Background: what's on screen now, briefly dimmed so the arriving window
    // stands out, and back to normal by the last frame.
    match kind {
        TransitionKind::Push => {
            // The background gives way slightly as the window pushes in.
            let give = 0.035 * (PI * t).sin();
            d2d.draw_bitmap(curr, transform(screen, 1.0 - give, dir * sw * give, 0.0), 1.0);
        }
        _ => d2d.draw_bitmap(curr, screen, 1.0),
    }
    let dim = 0.28 * (PI * t).sin();
    if dim > 0.005 {
        d2d.fill_rect(screen, black(dim));
    }

    let w = width(dest);
    let h = height(dest);
    match kind {
        TransitionKind::Slide | TransitionKind::Push => {
            // Travel in from the screen edge on the swipe side.
            let start = if dir > 0.0 { -dest.right } else { sw - dest.left };
            let r = transform(dest, 1.0, start * (1.0 - t), 0.0);
            let r = if matches!(kind, TransitionKind::Push) {
                transform(r, 0.9 + 0.1 * t, 0.0, 0.0)
            } else {
                r
            };
            shadow(d2d, r, 1.0);
            window(d2d, next, src, r, 1.0);
        }
        TransitionKind::Fade => {
            let r = transform(dest, 1.0, -dir * 36.0 * (1.0 - t), 0.0);
            shadow(d2d, r, t);
            window(d2d, next, src, r, t);
        }
        TransitionKind::Zoom => {
            // Grows out of the swipe side into place.
            let r = transform(dest, 0.7 + 0.3 * t, -dir * w * 0.25 * (1.0 - t), 0.0);
            let alpha = (t / 0.6).min(1.0);
            shadow(d2d, r, alpha);
            window(d2d, next, src, r, alpha);
        }
        TransitionKind::Stack => {
            // Rises from below, arriving from the swipe side.
            let dy = (sh - dest.top) * (1.0 - t);
            let dx = -dir * w * 0.3 * (1.0 - t);
            let r = transform(dest, 0.94 + 0.06 * t, dx, dy);
            shadow(d2d, r, 1.0);
            window(d2d, next, src, r, 1.0);
        }
        TransitionKind::Cube => {
            // Swings open like a door hinged on the swipe-side edge.
            let open = w * t;
            let r = if dir > 0.0 {
                D2D_RECT_F {
                    right: dest.left + open,
                    ..dest
                }
            } else {
                D2D_RECT_F {
                    left: dest.right - open,
                    ..dest
                }
            };
            shadow(d2d, r, t);
            window(d2d, next, src, r, 1.0);
            d2d.fill_rect(r, black(0.55 * (1.0 - t)));
        }
        TransitionKind::Flip => {
            // Turns face-on around its vertical axis, drifting in from the
            // swipe side.
            let r = D2D_RECT_F {
                top: dest.top,
                bottom: dest.bottom,
                ..transform(dest, 1.0, -dir * w * 0.12 * (1.0 - t), 0.0)
            };
            let cx = (r.left + r.right) * 0.5;
            let half = w * 0.5 * t;
            let r = D2D_RECT_F {
                left: cx - half,
                right: cx + half,
                ..r
            };
            shadow(d2d, r, t);
            window(d2d, next, src, r, 1.0);
            d2d.fill_rect(r, black(0.5 * (1.0 - t)));
        }
        TransitionKind::DuoFold => {
            // Two halves unfold outward from the middle.
            let cx = (dest.left + dest.right) * 0.5;
            let half = w * 0.5 * t;
            let shade = black(0.5 * (1.0 - t));
            shadow(d2d, D2D_RECT_F { left: cx - half, right: cx + half, ..dest }, t);
            let left = D2D_RECT_F { left: cx - half, right: cx, ..dest };
            let right = D2D_RECT_F { left: cx, right: cx + half, ..dest };
            window_slice(d2d, next, src, 0.0, 0.5, left, 1.0);
            window_slice(d2d, next, src, 0.5, 1.0, right, 1.0);
            d2d.fill_rect(left, shade);
            d2d.fill_rect(right, shade);
        }
        TransitionKind::OrbitThrow | TransitionKind::HexBloom => unreachable!(),
    }
    let _ = h;
}

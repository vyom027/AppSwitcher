//! Orbit Throw: the current window shrinks into a card and swings out around
//! a tilted orbit, while the next window swings in from the far side of the
//! orbit and grows back to full screen. Pairs with the Orbit picker layout.

use crate::render::d2d::D2DContext;
use std::f32::consts::PI;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

/// Card size (fraction of the screen) while the windows are in orbit.
const CARD_SCALE: f32 = 0.40;

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn black(a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a,
    }
}

/// Orbit geometry shared with the settings preview: (centre x, centre y of
/// the front of the orbit, horizontal radius, vertical radius).
pub fn orbit(sw: f32, sh: f32) -> (f32, f32, f32, f32) {
    (sw * 0.5, sh * 0.5, sw * 0.34, sh * 0.10)
}

/// Where a card sits on the orbit at angle `theta` (0 = front centre) and
/// its depth (1 = front, 0 = back).
pub fn orbit_point(theta: f32, sw: f32, sh: f32) -> (f32, f32, f32) {
    let (cx, cy, a, b) = orbit(sw, sh);
    let depth = (theta.cos() + 1.0) * 0.5;
    // The back of the orbit sits higher on screen, which reads as a tilt.
    (cx + a * theta.sin(), cy - b * (1.0 - theta.cos()), depth)
}

/// Per-card state at progress `t`: (theta, size before depth scaling).
pub fn cards(t: f32, direction: i32) -> [(f32, f32); 2] {
    let dir = if direction >= 0 { 1.0 } else { -1.0 };
    let shrink = smoothstep(0.0, 0.32, t);
    let spin = smoothstep(0.12, 0.88, t);
    let grow = smoothstep(0.68, 1.0, t);
    let theta_old = dir * PI * spin;
    let theta_new = theta_old - dir * PI;
    [
        (theta_old, 1.0 + (CARD_SCALE - 1.0) * shrink),
        (theta_new, CARD_SCALE + (1.0 - CARD_SCALE) * grow),
    ]
}

pub fn render(
    d2d: &D2DContext,
    curr: &ID2D1Bitmap,
    next: &ID2D1Bitmap,
    t: f32,
    direction: i32,
    sw: f32,
    sh: f32,
) {
    let full = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: sw,
        bottom: sh,
    };
    if t <= 0.0 {
        d2d.draw_bitmap(curr, full, 1.0);
        return;
    }
    if t >= 1.0 {
        d2d.draw_bitmap(next, full, 1.0);
        return;
    }

    // Faint orbit path while the cards travel.
    let (cx, cy, a, b) = orbit(sw, sh);
    let spin = smoothstep(0.12, 0.88, t);
    let ring_alpha = 0.22 * (spin * (1.0 - spin) * 4.0).min(1.0);
    if ring_alpha > 0.01 {
        let white = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: ring_alpha,
        };
        d2d.draw_ellipse(cx, cy - b, a, b, white, 1.5);
    }

    let [old, new] = cards(t, direction);
    let mut order = [(curr, old), (next, new)];
    // Back card first.
    order.sort_by(|x, y| x.1 .0.cos().total_cmp(&y.1 .0.cos()));

    for (bitmap, (theta, size)) in order {
        let (x, y, depth) = orbit_point(theta, sw, sh);
        let scale = size * (0.62 + 0.38 * depth);
        let w = sw * scale;
        let h = sh * scale;
        let rect = D2D_RECT_F {
            left: x - w * 0.5,
            top: y - h * 0.5,
            right: x + w * 0.5,
            bottom: y + h * 0.5,
        };
        // How "card-like" this frame is: 0 at full screen, 1 in orbit.
        let cardness = ((1.0 - size) / (1.0 - CARD_SCALE)).clamp(0.0, 1.0);
        let radius = 18.0 * cardness;
        if cardness > 0.01 {
            d2d.fill_rounded_rect(
                D2D_RECT_F {
                    left: rect.left - 4.0,
                    top: rect.top + 6.0,
                    right: rect.right + 4.0,
                    bottom: rect.bottom + 18.0,
                },
                radius + 6.0,
                black(0.35 * cardness),
            );
        }
        d2d.fill_rounded_bitmap(bitmap, rect, radius, 1.0);
        let dim = 0.55 * (1.0 - depth);
        if dim > 0.01 {
            d2d.fill_rounded_rect(rect, radius, black(dim));
        }
        if cardness > 0.01 {
            d2d.draw_rounded_rect(
                rect,
                radius,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.18 * cardness,
                },
                1.0,
            );
        }
    }
}

/// Windowed variant: the current screen stays put and the next window swings
/// in from the far side of an orbit around its own position, landing exactly
/// on its real rectangle.
#[allow(clippy::too_many_arguments)]
pub fn render_windowed(
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
    let screen = D2D_RECT_F {
        left: 0.0,
        top: 0.0,
        right: sw,
        bottom: sh,
    };
    d2d.draw_bitmap(curr, screen, 1.0);
    let dim = 0.28 * (PI * t).sin();
    if dim > 0.005 {
        d2d.fill_rect(screen, black(dim));
    }
    if t <= 0.0 {
        return;
    }

    let dir = if direction >= 0 { 1.0 } else { -1.0 };
    let (w, h) = (dest.right - dest.left, dest.bottom - dest.top);
    let (rcx, rcy) = ((dest.left + dest.right) * 0.5, (dest.top + dest.bottom) * 0.5);
    // Orbit around the window's own centre, wide enough to read as a swing.
    let a = (w * 0.75).max(sw * 0.22);
    let b = (h * 0.18).max(sh * 0.06);

    let spin = smoothstep(0.0, 0.85, t);
    let grow = smoothstep(0.55, 1.0, t);
    let appear = smoothstep(0.0, 0.18, t);
    let ring = 0.22 * (spin * (1.0 - spin) * 4.0).min(1.0);
    if ring > 0.01 {
        d2d.draw_ellipse(
            rcx,
            rcy - b,
            a,
            b,
            D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: ring,
            },
            1.5,
        );
    }

    // From the back of the orbit on the swipe side round to the front.
    let theta = -dir * PI * (1.0 - spin);
    let depth = (theta.cos() + 1.0) * 0.5;
    let size = (CARD_SCALE + (1.0 - CARD_SCALE) * grow) * (0.62 + 0.38 * depth);
    let x = rcx + a * theta.sin();
    let y = rcy - b * (1.0 - theta.cos());
    let r = D2D_RECT_F {
        left: x - w * size * 0.5,
        top: y - h * size * 0.5,
        right: x + w * size * 0.5,
        bottom: y + h * size * 0.5,
    };
    super::windowed::shadow(d2d, r, appear);
    d2d.draw_bitmap_subrect(next, r, src, appear);
    let shade = 0.55 * (1.0 - depth) * appear;
    if shade > 0.01 {
        d2d.fill_rect(r, black(shade));
    }
}

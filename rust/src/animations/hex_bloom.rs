//! Hex Bloom: the screen breaks into hexagon cells that turn over in a wave,
//! starting from the side the new window comes from, revealing it cell by
//! cell. Pairs with the Honeycomb picker layout.

use crate::render::d2d::{hexagon, D2DContext};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

/// Share of the timeline used to stagger the wave across the screen; each
/// cell's own turn takes the remaining share.
pub const SPREAD: f32 = 0.68;

/// Centres of pointy-top hexagons of circumradius `r` covering `w`×`h`.
pub fn cells(w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    let col_w = 3f32.sqrt() * r;
    let row_h = 1.5 * r;
    let cols = (w / col_w).ceil() as i32 + 1;
    let rows = (h / row_h).ceil() as i32 + 1;
    let mut out = Vec::with_capacity(((cols + 1) * (rows + 1)) as usize);
    for row in -1..=rows {
        let offset = if row.rem_euclid(2) == 1 { col_w * 0.5 } else { 0.0 };
        for col in -1..=cols {
            out.push((col as f32 * col_w + offset, row as f32 * row_h));
        }
    }
    out
}

/// Turn progress (0..1) of a cell centred at `c`, for overall progress `t`,
/// with the wave starting at `origin`.
pub fn cell_progress(c: (f32, f32), origin: (f32, f32), max_dist: f32, t: f32) -> f32 {
    let d = ((c.0 - origin.0).hypot(c.1 - origin.1) / max_dist).clamp(0.0, 1.0);
    ((t - d * SPREAD) / (1.0 - SPREAD)).clamp(0.0, 1.0)
}

/// Wave origin: the edge the new window comes from (matches Slide).
pub fn origin(direction: i32, w: f32, h: f32) -> (f32, f32) {
    (if direction > 0 { 0.0 } else { w }, h * 0.5)
}

fn black(a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a,
    }
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

    let r = (sh / 9.0).max(48.0);
    let origin = origin(direction, sw, sh);
    let max_dist = (sw).hypot(sh * 0.5);
    let (Some(curr_brush), Some(next_brush)) = (
        d2d.bitmap_brush(curr, full, 1.0),
        d2d.bitmap_brush(next, full, 1.0),
    ) else {
        d2d.draw_bitmap(if t < 0.5 { curr } else { next }, full, 1.0);
        return;
    };

    // Base: the old window everywhere, then finished cells show the new one.
    // Aliased fills tile hexagons without hairline seams between them.
    d2d.draw_bitmap(curr, full, 1.0);
    let mut turning = Vec::new();
    d2d.set_aliased(true);
    for c in cells(sw, sh, r) {
        let p = cell_progress(c, origin, max_dist, t);
        if p >= 1.0 {
            if let Some(g) = d2d.polygon(&hexagon(c.0, c.1, r, 1.0)) {
                d2d.fill_geometry_with(&g, &next_brush);
            }
        } else if p > 0.0 {
            turning.push((c, p));
        }
    }
    d2d.set_aliased(false);

    // Cells mid-turn: the face narrows to an edge, then the back opens up.
    for ((x, y), p) in turning {
        if let Some(slot) = d2d.polygon(&hexagon(x, y, r, 1.0)) {
            // Dim (not black out) the gap behind a turning cell, so the old
            // window still shows faintly through the wave.
            d2d.fill_geometry(&slot, black(0.72));
        }
        let (brush, sx) = if p < 0.5 {
            (&curr_brush, 1.0 - 2.0 * p)
        } else {
            (&next_brush, 2.0 * p - 1.0)
        };
        if sx < 0.02 {
            continue;
        }
        if let Some(face) = d2d.polygon(&hexagon(x, y, r * 0.97, sx)) {
            d2d.fill_geometry_with(&face, brush);
            // Shade the face as it turns edge-on, with a bright rim.
            let edge_on = 1.0 - sx;
            d2d.fill_geometry(&face, black(0.5 * edge_on));
            d2d.draw_geometry(
                &face,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35 * edge_on,
                },
                1.2,
            );
        }
    }
}

/// Windowed variant: the hexagon wave runs only inside the next window's real
/// rectangle, turning the current screen over into the window cell by cell.
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
    if t <= 0.0 {
        return;
    }
    let (w, h) = (dest.right - dest.left, dest.bottom - dest.top);
    let (src_w, src_h) = (src.right - src.left, src.bottom - src.top);
    if t >= 1.0 || w < 4.0 || h < 4.0 || src_w <= 0.0 || src_h <= 0.0 {
        d2d.draw_bitmap_subrect(next, dest, src, 1.0);
        return;
    }

    // Map the whole next bitmap so that `src` lands exactly on `dest`.
    let size = unsafe { next.GetSize() };
    let (kx, ky) = (w / src_w, h / src_h);
    let next_full = D2D_RECT_F {
        left: dest.left - src.left * kx,
        top: dest.top - src.top * ky,
        right: dest.left + (size.width - src.left) * kx,
        bottom: dest.top + (size.height - src.top) * ky,
    };
    let (Some(curr_brush), Some(next_brush)) = (
        d2d.bitmap_brush(curr, screen, 1.0),
        d2d.bitmap_brush(next, next_full, 1.0),
    ) else {
        d2d.draw_bitmap_subrect(next, dest, src, t);
        return;
    };

    let r = (h / 6.0).clamp(26.0, sh / 9.0);
    let local_origin = origin(direction, w, h);
    let max_dist = w.hypot(h * 0.5);
    d2d.push_clip(dest);
    let mut turning = Vec::new();
    d2d.set_aliased(true);
    for (cx, cy) in cells(w, h, r) {
        let p = cell_progress((cx, cy), local_origin, max_dist, t);
        let c = (dest.left + cx, dest.top + cy);
        if p >= 1.0 {
            if let Some(g) = d2d.polygon(&hexagon(c.0, c.1, r, 1.0)) {
                d2d.fill_geometry_with(&g, &next_brush);
            }
        } else if p > 0.0 {
            turning.push((c, p));
        }
    }
    d2d.set_aliased(false);
    for ((x, y), p) in turning {
        if let Some(slot) = d2d.polygon(&hexagon(x, y, r, 1.0)) {
            d2d.fill_geometry(&slot, black(0.72));
        }
        let (brush, sx) = if p < 0.5 {
            (&curr_brush, 1.0 - 2.0 * p)
        } else {
            (&next_brush, 2.0 * p - 1.0)
        };
        if sx < 0.02 {
            continue;
        }
        if let Some(face) = d2d.polygon(&hexagon(x, y, r * 0.97, sx)) {
            d2d.fill_geometry_with(&face, brush);
            let edge_on = 1.0 - sx;
            d2d.fill_geometry(&face, black(0.5 * edge_on));
            d2d.draw_geometry(
                &face,
                D2D1_COLOR_F {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.35 * edge_on,
                },
                1.2,
            );
        }
    }
    d2d.pop_clip();
}

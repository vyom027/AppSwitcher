//! Honeycomb: windows sit in hexagon tiles spiralling out from the centre in
//! most-recently-used order. Tiles never move; the selected one grows a
//! little and gets the accent outline. Navigation is spatial: Left/Right/Tab
//! move in reading order (along a row, then to the next row) and Up/Down move
//! to the nearest tile in the row above or below.

use crate::layouts::{style, PickerLayout};
use crate::render::d2d::{hexagon, D2DContext};
use crate::window::enumerator::WindowInfo;
use std::cell::RefCell;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

const SQRT3: f32 = 1.732_050_8;
/// How much the selected tile grows. Kept small so it never covers its
/// neighbours enough to get in the way of the mouse.
const SWELL: f32 = 0.16;

#[derive(Default)]
pub struct HoneycombLayout {
    /// Selection position in reading order (may be fractional while gliding).
    focus: f32,
    selected: usize,
    /// (window index, centre x, centre y, circumradius) from the last render,
    /// in draw order, for hit-testing.
    tiles: RefCell<Vec<(usize, f32, f32, f32)>>,
}

/// Axial hex coordinates in spiral order: centre, then ring 1, ring 2, ...
fn spiral(n: usize) -> Vec<(i32, i32)> {
    const DIRS: [(i32, i32); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
    let mut out = vec![(0, 0)];
    let mut k = 1;
    while out.len() < n {
        let mut pos = (DIRS[4].0 * k, DIRS[4].1 * k);
        for dir in DIRS {
            for _ in 0..k {
                out.push(pos);
                pos = (pos.0 + dir.0, pos.1 + dir.1);
            }
        }
        k += 1;
    }
    out.truncate(n);
    out
}

/// Position of an axial cell in units of the tile radius: (x, row).
fn unit_pos((q, r): (i32, i32)) -> (f32, i32) {
    (SQRT3 * (q as f32 + r as f32 * 0.5), r)
}

/// Window indices in reading order: top row first, left to right.
fn reading_order(n: usize) -> Vec<usize> {
    let cells = spiral(n);
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let (xa, ra) = unit_pos(cells[a]);
        let (xb, rb) = unit_pos(cells[b]);
        ra.cmp(&rb).then(xa.total_cmp(&xb))
    });
    order
}

/// Number of rings needed to hold `n` tiles (ring k adds 6k tiles).
fn rings_for(n: usize) -> usize {
    let mut k = 0;
    while 1 + 3 * k * (k + 1) < n {
        k += 1;
    }
    k
}

/// True when (px, py) is inside a pointy-top hexagon.
fn in_hexagon(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> bool {
    let dx = (px - cx).abs();
    let dy = (py - cy).abs();
    dx <= SQRT3 * 0.5 * r && dy <= r - dx / SQRT3
}

/// The tile in the row above (`dir` = -1) or below (+1) nearest to
/// `selected` horizontally, if there is such a row.
fn vertical_neighbour(n: usize, selected: usize, dir: i32) -> Option<usize> {
    let cells = spiral(n);
    let (x, row) = unit_pos(*cells.get(selected)?);
    (0..n)
        .filter(|&i| unit_pos(cells[i]).1 == row + dir)
        .min_by(|&a, &b| {
            let da = (unit_pos(cells[a]).0 - x).abs();
            let db = (unit_pos(cells[b]).0 - x).abs();
            da.total_cmp(&db)
        })
}

impl HoneycombLayout {
    /// Tile circumradius that fits all `n` windows on screen.
    fn radius(n: usize, sw: f32, sh: f32) -> f32 {
        let k = rings_for(n) as f32;
        let by_width = (sw * 0.42) / (SQRT3 * (k + 0.5));
        let by_height = (sh * 0.36) / (1.5 * k + 1.0);
        by_width.min(by_height).clamp(44.0, sh * 0.17)
    }
}

impl PickerLayout for HoneycombLayout {
    fn update(&mut self, focus: f32, selected: usize) {
        self.focus = focus;
        self.selected = selected;
    }

    fn visible_indices(&self, windows: &[WindowInfo]) -> Vec<usize> {
        reading_order(windows.len())
    }

    fn step_vertical(&self, selected: usize, dir: i32, windows: &[WindowInfo]) -> Option<usize> {
        vertical_neighbour(windows.len(), selected, dir)
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
        let r = Self::radius(n, sw, sh);
        let (ox, oy) = (sw * 0.5, sh * 0.46);
        let cells = spiral(n);
        let order = reading_order(n);
        let mut position = vec![0usize; n];
        for (pos, &idx) in order.iter().enumerate() {
            position[idx] = pos;
        }

        let mut tiles: Vec<(usize, f32, f32, f32, f32)> = (0..n)
            .map(|i| {
                let (ux, row) = unit_pos(cells[i]);
                let (x, y) = (ox + r * ux, oy + r * 1.5 * row as f32);
                // Grow smoothly as the focus glides onto this tile.
                let near = (1.0 - (position[i] as f32 - self.focus).abs()).max(0.0);
                let swell = near * near * (3.0 - 2.0 * near);
                (i, x, y, r * 0.94 * (1.0 + SWELL * swell), swell)
            })
            .collect();
        // Selected (and growing) tiles on top.
        tiles.sort_by(|a, b| {
            let sel_a = a.0 == self.selected;
            let sel_b = b.0 == self.selected;
            sel_a.cmp(&sel_b).then(a.4.total_cmp(&b.4))
        });

        for &(i, x, y, rr, swell) in &tiles {
            let Some(hex) = d2d.polygon(&hexagon(x, y, rr, 1.0)) else {
                continue;
            };
            let selected = i == self.selected;
            if let Some(shadow) = d2d.polygon(&hexagon(x, y + rr * 0.06, rr * 1.02, 1.0)) {
                d2d.fill_geometry(&shadow, style::rgba(0.0, 0.0, 0.0, 0.25 + 0.15 * swell));
            }
            d2d.fill_geometry(&hex, style::THUMB_BG);
            let has_thumb = matches!(bitmaps.get(i), Some(Some(_)));
            if let Some(Some(bmp)) = bitmaps.get(i) {
                // Show most of the window: a little wider than the hexagon,
                // so only the edges are trimmed; short windows leave a band
                // above and below, tall ones are trimmed top and bottom.
                let size = unsafe { bmp.GetSize() };
                let aspect = if size.height > 0.0 { size.width / size.height } else { 1.6 };
                let w = SQRT3 * rr * 1.3;
                let h = w / aspect.max(0.3);
                let dest = D2D_RECT_F {
                    left: x - w * 0.5,
                    top: y - h * 0.5,
                    right: x + w * 0.5,
                    bottom: y + h * 0.5,
                };
                let opacity = if selected { 1.0 } else { 0.62 + 0.38 * swell };
                if let Some(brush) = d2d.bitmap_brush(bmp, dest, opacity) {
                    d2d.fill_geometry_with(&hex, &brush);
                }
            }
            // App icon near the bottom of the tile, on its own backing tile;
            // large and centred when there's no preview to show.
            if let Some(Some(icon)) = icons.get(i) {
                let (s, icy) = if has_thumb {
                    ((rr * 0.42).clamp(18.0, 40.0), y + rr * 0.52)
                } else {
                    ((rr * 0.62).clamp(28.0, 64.0), y)
                };
                style::icon_tile(
                    d2d,
                    icon,
                    D2D_RECT_F {
                        left: x - s * 0.5,
                        top: icy - s * 0.5,
                        right: x + s * 0.5,
                        bottom: icy + s * 0.5,
                    },
                );
            }
            if selected {
                if let Some(ring) = d2d.polygon(&hexagon(x, y, rr + 5.0, 1.0)) {
                    d2d.draw_geometry(&ring, accent, 3.0);
                }
            } else {
                d2d.draw_geometry(&hex, style::rgba(1.0, 1.0, 1.0, 0.10), 1.0);
            }
        }

        if let Some(w) = windows.get(self.selected) {
            style::title_pill(
                d2d,
                &w.title,
                icons.get(self.selected).and_then(Option::as_ref),
                sw * 0.5,
                sh - 70.0,
                36.0,
                (sw * 0.6).min(640.0),
            );
        }

        *self.tiles.borrow_mut() = tiles.iter().map(|&(i, x, y, rr, _)| (i, x, y, rr)).collect();
    }

    fn hit_test(&self, px: f32, py: f32, _windows: &[WindowInfo], _sw: f32, _sh: f32) -> Option<usize> {
        self.tiles
            .borrow()
            .iter()
            .rev()
            .find(|&&(_, x, y, rr)| in_hexagon(px, py, x, y, rr))
            .map(|t| t.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spiral_fills_rings_without_repeats() {
        let cells = spiral(19); // centre + ring 1 (6) + ring 2 (12)
        assert_eq!(cells[0], (0, 0));
        let unique: std::collections::HashSet<_> = cells.iter().collect();
        assert_eq!(unique.len(), 19);
        for &(q, r) in &cells[1..7] {
            let s = -q - r;
            assert_eq!(q.abs().max(r.abs()).max(s.abs()), 1);
        }
    }

    #[test]
    fn rings_cover_window_counts() {
        assert_eq!(rings_for(1), 0);
        assert_eq!(rings_for(7), 1);
        assert_eq!(rings_for(8), 2);
        assert_eq!(rings_for(19), 2);
    }

    #[test]
    fn hexagon_hit_test() {
        assert!(in_hexagon(0.0, 0.0, 0.0, 0.0, 10.0));
        assert!(in_hexagon(0.0, 9.9, 0.0, 0.0, 10.0));
        assert!(!in_hexagon(9.0, 9.0, 0.0, 0.0, 10.0));
    }

    #[test]
    fn reading_order_goes_row_by_row_left_to_right() {
        let n = 7; // centre + one ring: rows of 2, 3, 2
        let cells = spiral(n);
        let order = reading_order(n);
        assert_eq!(order.len(), n);
        for pair in order.windows(2) {
            let (xa, ra) = unit_pos(cells[pair[0]]);
            let (xb, rb) = unit_pos(cells[pair[1]]);
            assert!(ra < rb || (ra == rb && xa < xb));
        }
        // The centre (most recent) tile is the middle of the middle row.
        assert_eq!(order[3], 0);
    }

    #[test]
    fn up_and_down_move_between_rows() {
        let n = 7;
        // From the centre, up and down both land on a tile, in the right row.
        let up = vertical_neighbour(n, 0, -1).unwrap();
        let down = vertical_neighbour(n, 0, 1).unwrap();
        let cells = spiral(n);
        assert_eq!(unit_pos(cells[up]).1, -1);
        assert_eq!(unit_pos(cells[down]).1, 1);
        // No row above the top row.
        assert_eq!(vertical_neighbour(n, up, -1), None);
    }
}

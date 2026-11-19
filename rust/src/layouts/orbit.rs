//! Orbit: the selected window sits large in the centre while the others
//! circle it on a tilted 3D ring. Moving the selection spins the ring; the
//! window reaching the front of the ring is the one shown in the centre.

use crate::layouts::{style, PickerLayout};
use crate::window::enumerator::WindowInfo;
use crate::render::d2d::D2DContext;
use std::cell::{Cell, RefCell};
use std::f32::consts::{PI, TAU};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

#[derive(Default)]
pub struct OrbitLayout {
    focus: f32,
    selected: usize,
    /// Geometry from the last render, used for hit-testing.
    hero: Cell<Option<D2D_RECT_F>>,
    /// (window index, rect, depth, visibility) of ring cards, last render.
    cards: RefCell<Vec<(usize, D2D_RECT_F, f32, f32)>>,
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn aspect_of(bitmap: Option<&ID2D1Bitmap>) -> f32 {
    bitmap
        .map(|b| unsafe { b.GetSize() })
        .filter(|s| s.width > 0.0 && s.height > 0.0)
        .map(|s| s.width / s.height)
        .unwrap_or(1.6)
        .clamp(1.1, 2.4)
}

impl OrbitLayout {
    /// Angle between neighbouring cards: evenly spread for a few windows,
    /// never so tight that cards pile up.
    fn step(n: usize) -> f32 {
        (TAU / n.max(6) as f32).max(0.30)
    }

    /// Ring centre and radii.
    fn ring(sw: f32, sh: f32) -> (f32, f32, f32, f32) {
        (sw * 0.5, sh * 0.47, sw * 0.40, sh * 0.29)
    }

    fn hero_rect(&self, bitmaps: &[Option<ID2D1Bitmap>], sw: f32, sh: f32) -> D2D_RECT_F {
        let aspect = aspect_of(bitmaps.get(self.selected).and_then(Option::as_ref));
        let (max_w, max_h) = (sw * 0.44, sh * 0.46);
        let (w, h) = if max_w / aspect <= max_h {
            (max_w, max_w / aspect)
        } else {
            (max_h * aspect, max_h)
        };
        let (cx, cy) = (sw * 0.5, sh * 0.43);
        D2D_RECT_F {
            left: cx - w * 0.5,
            top: cy - h * 0.5,
            right: cx + w * 0.5,
            bottom: cy + h * 0.5,
        }
    }

    fn draw_card(
        d2d: &D2DContext,
        rect: D2D_RECT_F,
        depth: f32,
        alpha: f32,
        bitmap: Option<&ID2D1Bitmap>,
        icon: Option<&ID2D1Bitmap>,
    ) {
        let radius = 8.0;
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: rect.left - 2.0,
                top: rect.top + 4.0,
                right: rect.right + 2.0,
                bottom: rect.bottom + 10.0,
            },
            radius + 3.0,
            style::rgba(0.0, 0.0, 0.0, 0.30 * alpha),
        );
        d2d.fill_rounded_rect(rect, radius, D2D1_COLOR_F { a: alpha, ..style::THUMB_BG });
        if let Some(bitmap) = bitmap {
            d2d.fill_rounded_bitmap(bitmap, rect, radius, alpha);
        }
        // Cards further back sink into shadow.
        let dim = 0.6 * (1.0 - depth) * alpha;
        if dim > 0.01 {
            d2d.fill_rounded_rect(rect, radius, style::rgba(0.0, 0.0, 0.0, dim));
        }
        d2d.draw_rounded_rect(rect, radius, style::rgba(1.0, 1.0, 1.0, 0.10 * alpha), 1.0);
        let w = rect.right - rect.left;
        if let (Some(icon), true) = (icon, w > 110.0 && alpha > 0.5) {
            let s = (w * 0.16).clamp(20.0, 30.0);
            style::icon_tile(
                d2d,
                icon,
                D2D_RECT_F {
                    left: rect.left + 6.0,
                    top: rect.top + 6.0,
                    right: rect.left + 6.0 + s,
                    bottom: rect.top + 6.0 + s,
                },
            );
        }
    }
}

impl PickerLayout for OrbitLayout {
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
        let (cx, cy, a, b) = Self::ring(sw, sh);
        let step = Self::step(n);

        // Orbit guides
        d2d.draw_ellipse(cx, cy, a, b, style::rgba(1.0, 1.0, 1.0, 0.07), 1.0);
        d2d.draw_ellipse(cx, cy, a * 1.04, b * 1.06, style::rgba(1.0, 1.0, 1.0, 0.035), 1.0);

        let base_w = (sw * 0.15).clamp(140.0, 280.0);
        let mut cards = Vec::with_capacity(n);
        for i in 0..n {
            let theta = (i as f32 - self.focus) * step;
            if theta.abs() > PI * 0.98 {
                continue; // beyond the ring; comes round as you spin
            }
            let depth = (theta.cos() + 1.0) * 0.5;
            // The window at the front is shown large in the centre, so its
            // ring card fades out as it arrives there.
            let alpha = smoothstep(0.12, step * 0.85, theta.abs());
            if alpha < 0.02 {
                continue;
            }
            let scale = 0.55 + 0.45 * depth;
            let aspect = aspect_of(bitmaps.get(i).and_then(Option::as_ref));
            let (w, h) = (base_w * scale, base_w * scale / aspect);
            let (x, y) = (cx + a * theta.sin(), cy + b * theta.cos());
            let rect = D2D_RECT_F {
                left: x - w * 0.5,
                top: y - h * 0.5,
                right: x + w * 0.5,
                bottom: y + h * 0.5,
            };
            cards.push((i, rect, depth, alpha));
        }
        cards.sort_by(|p, q| p.2.total_cmp(&q.2));

        let hero = self.hero_rect(bitmaps, sw, sh);
        let draw = |&(i, rect, depth, alpha): &(usize, D2D_RECT_F, f32, f32)| {
            Self::draw_card(
                d2d,
                rect,
                depth,
                alpha,
                bitmaps.get(i).and_then(Option::as_ref),
                icons.get(i).and_then(Option::as_ref),
            );
        };
        // Back half of the ring passes behind the centre window.
        for card in cards.iter().filter(|c| c.2 < 0.5) {
            draw(card);
        }

        style::shadow(d2d, hero, 12.0);
        d2d.fill_rounded_rect(hero, 12.0, style::THUMB_BG);
        if let Some(Some(bmp)) = bitmaps.get(self.selected) {
            d2d.fill_rounded_bitmap(bmp, hero, 12.0, 1.0);
        }
        style::selection(d2d, hero, 12.0, accent);
        if let Some(Some(icon)) = icons.get(self.selected) {
            style::icon_tile(
                d2d,
                icon,
                D2D_RECT_F {
                    left: hero.left + 14.0,
                    top: hero.top + 14.0,
                    right: hero.left + 50.0,
                    bottom: hero.top + 50.0,
                },
            );
        }

        for card in cards.iter().filter(|c| c.2 >= 0.5) {
            draw(card);
        }

        if let Some(w) = windows.get(self.selected) {
            style::title_pill(
                d2d,
                &w.title,
                icons.get(self.selected).and_then(Option::as_ref),
                sw * 0.5,
                hero.bottom + 18.0,
                36.0,
                (sw * 0.6).min(640.0),
            );
        }

        self.hero.set(Some(hero));
        *self.cards.borrow_mut() = cards;
    }

    fn hit_test(&self, px: f32, py: f32, _windows: &[WindowInfo], _sw: f32, _sh: f32) -> Option<usize> {
        let inside = |r: &D2D_RECT_F| px >= r.left && px <= r.right && py >= r.top && py <= r.bottom;
        let cards = self.cards.borrow();
        // Front cards are drawn over the centre window, so test them first.
        let front = cards
            .iter()
            .rev()
            .filter(|c| c.2 >= 0.5 && c.3 > 0.3)
            .find(|c| inside(&c.1));
        if let Some(c) = front {
            return Some(c.0);
        }
        if self.hero.get().is_some_and(|h| inside(&h)) {
            return Some(self.selected);
        }
        cards
            .iter()
            .rev()
            .filter(|c| c.2 < 0.5 && c.3 > 0.3)
            .find(|c| inside(&c.1))
            .map(|c| c.0)
    }
}

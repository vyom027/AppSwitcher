use crate::layouts::{draw_contained_thumbnail, style, PickerLayout};
use crate::render::d2d::Align;
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

/// A freeform window overview. The selected window is the large front card;
/// recent neighbors float around it in a deliberately non-grid arrangement.
pub struct ExposeCloudLayout {
    selected: usize,
}

impl Default for ExposeCloudLayout {
    fn default() -> Self {
        Self { selected: 0 }
    }
}

impl ExposeCloudLayout {
    const POSITIONS: [(f32, f32); 10] = [
        (-0.39, -0.24),
        (-0.13, -0.38),
        (0.17, -0.37),
        (0.40, -0.20),
        (-0.42, 0.10),
        (0.42, 0.11),
        (-0.38, 0.34),
        (-0.13, 0.40),
        (0.16, 0.40),
        (0.40, 0.33),
    ];

    fn neighbors(selected: usize, n: usize) -> Vec<usize> {
        let mut neighbors = Vec::with_capacity(Self::POSITIONS.len());
        for distance in 1..n {
            let next = selected.saturating_add(distance);
            if next < n {
                neighbors.push(next);
            }
            if selected >= distance {
                neighbors.push(selected - distance);
            }
            if neighbors.len() >= Self::POSITIONS.len() {
                break;
            }
        }
        neighbors
    }

    fn card_rect(slot: usize, sw: f32, sh: f32) -> D2D_RECT_F {
        let (x, y) = Self::POSITIONS[slot % Self::POSITIONS.len()];
        let w = (sw * 0.185).clamp(155.0, 290.0);
        let h = (sh * 0.165).clamp(105.0, 190.0);
        let cx = sw * (0.5 + x);
        let cy = sh * (0.49 + y);
        D2D_RECT_F {
            left: cx - w * 0.5,
            top: cy - h * 0.5,
            right: cx + w * 0.5,
            bottom: cy + h * 0.5,
        }
    }

    fn hero_rect(sw: f32, sh: f32) -> D2D_RECT_F {
        let w = (sw * 0.46).clamp(420.0, 860.0).min(sw * 0.64);
        let h = (sh * 0.55).clamp(280.0, 620.0).min(sh * 0.68);
        D2D_RECT_F {
            left: (sw - w) * 0.5,
            top: sh * 0.47 - h * 0.5,
            right: (sw + w) * 0.5,
            bottom: sh * 0.47 + h * 0.5,
        }
    }
}

impl PickerLayout for ExposeCloudLayout {
    fn update(&mut self, _focus: f32, selected: usize) {
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
        if windows.is_empty() {
            return;
        }
        let neighbors = Self::neighbors(self.selected, windows.len());
        for (slot, &idx) in neighbors.iter().enumerate() {
            let r = Self::card_rect(slot, sw, sh);
            style::surface(d2d, r, 8.0, false);
            draw_contained_thumbnail(
                d2d,
                D2D_RECT_F {
                    left: r.left + 4.0,
                    top: r.top + 4.0,
                    right: r.right - 4.0,
                    bottom: r.bottom - 29.0,
                },
                bitmaps.get(idx).and_then(Option::as_ref),
                0.82,
            );
            if let Some(Some(icon)) = icons.get(idx) {
                d2d.draw_bitmap(
                    icon,
                    D2D_RECT_F {
                        left: r.left + 9.0,
                        top: r.bottom - 24.0,
                        right: r.left + 25.0,
                        bottom: r.bottom - 8.0,
                    },
                    1.0,
                );
            }
            style::label(
                d2d,
                &windows[idx].title,
                D2D_RECT_F {
                    left: r.left + 32.0,
                    top: r.bottom - 28.0,
                    right: r.right - 8.0,
                    bottom: r.bottom - 4.0,
                },
                style::TEXT2,
                12.0,
                400,
                Align::Leading,
            );
        }

        let hero = Self::hero_rect(sw, sh);
        style::surface(d2d, hero, 10.0, true);
        draw_contained_thumbnail(
            d2d,
            D2D_RECT_F {
                left: hero.left + 5.0,
                top: hero.top + 5.0,
                right: hero.right - 5.0,
                bottom: hero.bottom - 48.0,
            },
            bitmaps.get(self.selected).and_then(Option::as_ref),
            1.0,
        );
        if let Some(Some(icon)) = icons.get(self.selected) {
            d2d.draw_bitmap(
                icon,
                D2D_RECT_F {
                    left: hero.left + 16.0,
                    top: hero.bottom - 37.0,
                    right: hero.left + 38.0,
                    bottom: hero.bottom - 15.0,
                },
                1.0,
            );
        }
        style::label(
            d2d,
            &windows[self.selected].title,
            D2D_RECT_F {
                left: hero.left + 48.0,
                top: hero.bottom - 44.0,
                right: hero.right - 18.0,
                bottom: hero.bottom - 8.0,
            },
            style::TEXT1,
            15.0,
            600,
            Align::Leading,
        );
        style::selection(d2d, hero, 10.0, accent);
        style::label(
            d2d,
            "Exposé",
            D2D_RECT_F {
                left: 28.0,
                top: 16.0,
                right: 250.0,
                bottom: 40.0,
            },
            style::TEXT2,
            13.0,
            600,
            Align::Leading,
        );
        style::label(
            d2d,
            &format!("{} of {} windows", neighbors.len() + 1, windows.len()),
            D2D_RECT_F {
                left: sw - 300.0,
                top: 16.0,
                right: sw - 28.0,
                bottom: 40.0,
            },
            style::TEXT3,
            12.0,
            400,
            Align::Trailing,
        );
    }

    fn hit_test(
        &self,
        px: f32,
        py: f32,
        windows: &[WindowInfo],
        sw: f32,
        sh: f32,
    ) -> Option<usize> {
        if windows.is_empty() {
            return None;
        }
        let hero = Self::hero_rect(sw, sh);
        if px >= hero.left && px <= hero.right && py >= hero.top && py <= hero.bottom {
            return Some(self.selected);
        }
        Self::neighbors(self.selected, windows.len())
            .into_iter()
            .enumerate()
            .find_map(|(slot, idx)| {
                let r = Self::card_rect(slot, sw, sh);
                (px >= r.left && px <= r.right && py >= r.top && py <= r.bottom).then_some(idx)
            })
    }
}

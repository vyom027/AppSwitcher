use crate::layouts::{draw_contained_thumbnail, style, PickerLayout};
use crate::render::d2d::{glyph, Align};
use crate::render::d2d::D2DContext;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

/// Search-first switcher. Focus is an ordinal within the filtered result list;
/// selected remains the source window index used by the rest of the picker.
pub struct SpotlightLayout {
    focus: f32,
    selected: usize,
    query: String,
}

impl Default for SpotlightLayout {
    fn default() -> Self {
        Self {
            focus: 0.0,
            selected: 0,
            query: String::new(),
        }
    }
}

impl SpotlightLayout {
    fn panel(sw: f32, sh: f32) -> D2D_RECT_F {
        let w = (sw * 0.84).min(1320.0).max(500.0).min(sw - 32.0);
        let h = (sh * 0.78).min(780.0).max(380.0).min(sh - 32.0);
        D2D_RECT_F {
            left: (sw - w) * 0.5,
            top: (sh - h) * 0.5,
            right: (sw + w) * 0.5,
            bottom: (sh + h) * 0.5,
        }
    }

    fn matches(&self, windows: &[WindowInfo]) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        windows
            .iter()
            .enumerate()
            .filter_map(|(i, w)| w.title.to_lowercase().contains(&query).then_some(i))
            .collect()
    }

    fn list_geometry(panel: D2D_RECT_F) -> (D2D_RECT_F, f32, usize) {
        let list = D2D_RECT_F {
            left: panel.left + 24.0,
            top: panel.top + 99.0,
            right: panel.left + (panel.right - panel.left) * 0.39,
            bottom: panel.bottom - 26.0,
        };
        let rows = (((list.bottom - list.top) / 67.0).floor() as usize).clamp(3, 8);
        let row_h = (list.bottom - list.top) / rows as f32;
        (list, row_h, rows)
    }
}

impl PickerLayout for SpotlightLayout {
    fn update(&mut self, focus: f32, selected: usize) {
        self.focus = focus;
        self.selected = selected;
    }
    fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
    }
    fn visible_indices(&self, windows: &[WindowInfo]) -> Vec<usize> {
        self.matches(windows)
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
        let panel = Self::panel(sw, sh);
        let ph = panel.bottom - panel.top;
        style::surface(d2d, panel, 12.0, true);

        style::label(
            d2d,
            "Spotlight",
            D2D_RECT_F {
                left: panel.left + 26.0,
                top: panel.top + 17.0,
                right: panel.left + 220.0,
                bottom: panel.top + 43.0,
            },
            style::TEXT1,
            14.0,
            600,
            Align::Leading,
        );
        style::label(
            d2d,
            "Find a window",
            D2D_RECT_F {
                left: panel.right - 220.0,
                top: panel.top + 19.0,
                right: panel.right - 26.0,
                bottom: panel.top + 42.0,
            },
            style::TEXT3,
            12.0,
            400,
            Align::Trailing,
        );

        // Fluent search box: subtle fill, accent underline while focused.
        let search = D2D_RECT_F {
            left: panel.left + 24.0,
            top: panel.top + 52.0,
            right: panel.right - 24.0,
            bottom: panel.top + 88.0,
        };
        d2d.fill_rounded_rect(search, 6.0, style::rgba(0.0, 0.0, 0.0, 0.22));
        d2d.draw_rounded_rect(search, 6.0, style::STROKE, 1.0);
        d2d.fill_rounded_rect(
            D2D_RECT_F {
                left: search.left + 1.0,
                top: search.bottom - 2.0,
                right: search.right - 1.0,
                bottom: search.bottom,
            },
            1.0,
            accent,
        );
        d2d.icon(
            glyph::SEARCH,
            D2D_RECT_F {
                left: search.left + 10.0,
                right: search.left + 34.0,
                ..search
            },
            style::TEXT2,
            13.0,
        );
        let query_text = if self.query.is_empty() {
            "Type to filter by window title…"
        } else {
            &self.query
        };
        style::label(
            d2d,
            query_text,
            D2D_RECT_F {
                left: search.left + 40.0,
                right: search.right - 60.0,
                ..search
            },
            if self.query.is_empty() { style::TEXT3 } else { style::TEXT1 },
            14.0,
            400,
            Align::Leading,
        );
        let esc = D2D_RECT_F {
            left: search.right - 46.0,
            top: search.top + 7.0,
            right: search.right - 10.0,
            bottom: search.bottom - 7.0,
        };
        d2d.fill_rounded_rect(esc, 4.0, style::rgba(1.0, 1.0, 1.0, 0.08));
        style::label(d2d, "Esc", esc, style::TEXT2, 11.0, 600, Align::Center);

        let matches = self.matches(windows);
        let (list, row_h, rows) = Self::list_geometry(panel);
        let focus = (self.focus.round() as usize).min(matches.len().saturating_sub(1));
        let start = focus
            .saturating_sub(rows / 2)
            .min(matches.len().saturating_sub(rows));
        for (row, (_absolute, &idx)) in matches
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .enumerate()
        {
            let r = D2D_RECT_F {
                left: list.left,
                top: list.top + row as f32 * row_h + 3.0,
                right: list.right,
                bottom: list.top + (row as f32 + 1.0) * row_h - 3.0,
            };
            let active = idx == self.selected;
            // Fluent list selection: soft fill plus an accent pill on the left.
            if active {
                d2d.fill_rounded_rect(r, 6.0, style::rgba(1.0, 1.0, 1.0, 0.08));
                let cy = (r.top + r.bottom) * 0.5;
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: r.left,
                        top: cy - 9.0,
                        right: r.left + 3.0,
                        bottom: cy + 9.0,
                    },
                    1.5,
                    accent,
                );
            }
            if let Some(Some(icon)) = icons.get(idx) {
                let cy = (r.top + r.bottom) * 0.5;
                d2d.draw_bitmap(
                    icon,
                    D2D_RECT_F {
                        left: r.left + 14.0,
                        top: cy - 12.0,
                        right: r.left + 38.0,
                        bottom: cy + 12.0,
                    },
                    1.0,
                );
            }
            style::label(
                d2d,
                &windows[idx].title,
                D2D_RECT_F {
                    left: r.left + 49.0,
                    right: r.right - 9.0,
                    ..r
                },
                if active { style::TEXT1 } else { style::TEXT2 },
                13.0,
                if active { 600 } else { 400 },
                Align::Leading,
            );
        }
        if matches.is_empty() {
            d2d.draw_text(
                "No matching windows",
                D2D_RECT_F {
                    left: list.left + 8.0,
                    top: list.top + 15.0,
                    right: list.right - 8.0,
                    bottom: list.top + 62.0,
                },
                D2D1_COLOR_F {
                    r: 0.72,
                    g: 0.77,
                    b: 0.86,
                    a: 0.92,
                },
                14.0,
                true,
                false,
            );
            d2d.draw_text(
                "Backspace to edit  ·  Esc to cancel",
                D2D_RECT_F {
                    left: list.left + 8.0,
                    top: list.top + 54.0,
                    right: list.right - 8.0,
                    bottom: list.top + 86.0,
                },
                D2D1_COLOR_F {
                    r: 0.55,
                    g: 0.62,
                    b: 0.74,
                    a: 0.85,
                },
                11.0,
                false,
                false,
            );
        }

        let preview = D2D_RECT_F {
            left: list.right + 22.0,
            top: list.top,
            right: panel.right - 24.0,
            bottom: panel.bottom - 26.0,
        };
        d2d.fill_rounded_rect(preview, 8.0, style::rgba(0.0, 0.0, 0.0, 0.18));
        d2d.draw_rounded_rect(preview, 8.0, style::STROKE, 1.0);
        if let Some(selected) = matches.get(focus).copied() {
            let header_h = 54.0;
            style::label(
                d2d,
                &windows[selected].title,
                D2D_RECT_F {
                    left: preview.left + 16.0,
                    top: preview.top + 11.0,
                    right: preview.right - 16.0,
                    bottom: preview.top + 39.0,
                },
                style::TEXT1,
                15.0,
                600,
                Align::Leading,
            );
            draw_contained_thumbnail(
                d2d,
                D2D_RECT_F {
                    left: preview.left + 6.0,
                    top: preview.top + header_h,
                    right: preview.right - 6.0,
                    bottom: preview.bottom - 6.0,
                },
                bitmaps.get(selected).and_then(Option::as_ref),
                1.0,
            );
        } else {
            d2d.draw_text(
                "Your preview appears here",
                D2D_RECT_F {
                    left: preview.left + 25.0,
                    top: preview.top + ph * 0.45,
                    right: preview.right - 25.0,
                    bottom: preview.top + ph * 0.55,
                },
                D2D1_COLOR_F {
                    r: 0.58,
                    g: 0.65,
                    b: 0.77,
                    a: 0.9,
                },
                13.0,
                false,
                true,
            );
        }
        let count = format!(
            "{} {}",
            matches.len(),
            if matches.len() == 1 { "match" } else { "matches" }
        );
        d2d.draw_text(
            &count,
            D2D_RECT_F {
                left: panel.left + 26.0,
                top: panel.bottom - 23.0,
                right: panel.left + 240.0,
                bottom: panel.bottom - 8.0,
            },
            style::TEXT3,
            11.5,
            false,
            false,
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
        let matches = self.matches(windows);
        if matches.is_empty() {
            return None;
        }
        let panel = Self::panel(sw, sh);
        let (list, row_h, rows) = Self::list_geometry(panel);
        let focus = (self.focus.round() as usize).min(matches.len() - 1);
        let start = focus
            .saturating_sub(rows / 2)
            .min(matches.len().saturating_sub(rows));
        if px >= list.left && px <= list.right && py >= list.top && py <= list.bottom {
            let row = ((py - list.top) / row_h).floor() as usize;
            if let Some(&idx) = matches.get(start + row) {
                return Some(idx);
            }
        }
        let preview_left = list.right + 22.0;
        if px >= preview_left
            && px <= panel.right - 24.0
            && py >= list.top
            && py <= panel.bottom - 26.0
        {
            return matches.get(focus).copied();
        }
        None
    }
}

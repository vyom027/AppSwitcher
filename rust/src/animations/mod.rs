pub mod duo_fold;
pub mod slide;

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
            _ => Self::Slide,
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
            Self::Fade => {
                let rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                };
                d2d.draw_bitmap(curr, rect, (1.0 - t).max(0.0));
                d2d.draw_bitmap(next, rect, t.min(1.0));
            }
            Self::Zoom => {
                let curr_s = 1.0 + 0.18 * t;
                let next_s = 0.82 + 0.18 * t;
                let c_rect = D2D_RECT_F {
                    left: (sw - sw * curr_s) * 0.5,
                    top: (sh - sh * curr_s) * 0.5,
                    right: (sw + sw * curr_s) * 0.5,
                    bottom: (sh + sh * curr_s) * 0.5,
                };
                let n_rect = D2D_RECT_F {
                    left: (sw - sw * next_s) * 0.5,
                    top: (sh - sh * next_s) * 0.5,
                    right: (sw + sw * next_s) * 0.5,
                    bottom: (sh + sh * next_s) * 0.5,
                };
                d2d.draw_bitmap(curr, c_rect, (1.0 - t).max(0.0));
                d2d.draw_bitmap(next, n_rect, t.min(1.0));
            }
            Self::Push => {
                // Static current app slightly dimming
                let full_rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                };
                d2d.draw_bitmap(curr, full_rect, 1.0 - 0.30 * t);

                // Next app slides over from the side with a deep drop shadow
                let dir = direction as f32;
                let next_x = -dir * (1.0 - t) * sw;
                let next_rect = D2D_RECT_F {
                    left: next_x,
                    top: 0.0,
                    right: next_x + sw,
                    bottom: sh,
                };

                // Drop shadow along leading edge
                let shadow_x = if dir > 0.0 { next_x + sw } else { next_x - 30.0 };
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: shadow_x,
                        top: 0.0,
                        right: shadow_x + 30.0,
                        bottom: sh,
                    },
                    0.0,
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.50 * (1.0 - t),
                    },
                );

                d2d.draw_bitmap(next, next_rect, 1.0);
            }
            Self::Stack => {
                // Current app dims as next app rises
                let full_rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                };
                d2d.draw_bitmap(curr, full_rect, 1.0 - 0.40 * t);

                // Next app rises up from bottom
                let next_y = (1.0 - t) * sh;
                let next_rect = D2D_RECT_F {
                    left: 0.0,
                    top: next_y,
                    right: sw,
                    bottom: next_y + sh,
                };

                // Ambient shadow above rising card
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: 0.0,
                        top: next_y - 24.0,
                        right: sw,
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
            Self::Cube | Self::Flip => {
                // 3D perspective panel flip
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

                let dir = direction as f32;
                if t < 0.5 {
                    let sub_t = t / 0.5;
                    let angle = sub_t * 85.0f32.to_radians();
                    let cos = angle.cos();
                    let sin = angle.sin();
                    let scale_x = cos;
                    let scale_y = 1.0 - sin * 0.22;
                    let w = sw * scale_x;
                    let h = sh * scale_y;
                    let cx = sw * 0.5 - dir * (1.0 - cos) * sw * 0.25;
                    let cy = sh * 0.5;
                    let rect = D2D_RECT_F {
                        left: cx - w * 0.5,
                        top: cy - h * 0.5,
                        right: cx + w * 0.5,
                        bottom: cy + h * 0.5,
                    };
                    d2d.draw_bitmap(curr, rect, 1.0 - sub_t * 0.4);
                } else {
                    let sub_t = (t - 0.5) / 0.5;
                    let angle = (1.0 - sub_t) * 85.0f32.to_radians();
                    let cos = angle.cos();
                    let sin = angle.sin();
                    let scale_x = cos;
                    let scale_y = 1.0 - sin * 0.22;
                    let w = sw * scale_x;
                    let h = sh * scale_y;
                    let cx = sw * 0.5 + dir * (1.0 - cos) * sw * 0.25;
                    let cy = sh * 0.5;
                    let rect = D2D_RECT_F {
                        left: cx - w * 0.5,
                        top: cy - h * 0.5,
                        right: cx + w * 0.5,
                        bottom: cy + h * 0.5,
                    };
                    d2d.draw_bitmap(next, rect, 0.6 + sub_t * 0.4);
                }
            }
        }
    }
}

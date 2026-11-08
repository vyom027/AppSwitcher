pub mod duo_fold;
pub mod slide;

pub use duo_fold::DuoFoldTransition;
pub use slide::SlideTransition;

use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub enum TransitionKind {
    Slide,
    DuoFold,
    Fade,
    Zoom,
}

impl TransitionKind {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "duo_fold" | "duo fold" => Self::DuoFold,
            "fade" => Self::Fade,
            "zoom" => Self::Zoom,
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
                d2d.draw_bitmap(curr, rect, 1.0 - t);
                d2d.draw_bitmap(next, rect, t);
            }
            Self::Zoom => {
                let _rect = D2D_RECT_F {
                    left: 0.0,
                    top: 0.0,
                    right: sw,
                    bottom: sh,
                };
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
                d2d.draw_bitmap(curr, c_rect, 1.0 - t);
                d2d.draw_bitmap(next, n_rect, t);
            }
        }
    }
}


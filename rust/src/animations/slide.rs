use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_RECT_F};
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;

pub struct SlideTransition {
    pub direction: i32,
}

impl SlideTransition {
    pub fn new(direction: i32) -> Self {
        Self { direction }
    }

    pub fn render(
        &self,
        d2d: &D2DContext,
        curr_bitmap: &ID2D1Bitmap,
        next_bitmap: &ID2D1Bitmap,
        t: f32,
        sw: f32,
        sh: f32,
    ) {
        let dir = self.direction as f32;
        let curr_x = dir * t * sw;
        let next_x = -dir * (1.0 - t) * sw;

        self.draw_slide_panel(d2d, curr_bitmap, curr_x, sw, sh);
        self.draw_slide_panel(d2d, next_bitmap, next_x, sw, sh);
    }

    fn draw_slide_panel(
        &self,
        d2d: &D2DContext,
        bitmap: &ID2D1Bitmap,
        offset_x: f32,
        sw: f32,
        sh: f32,
    ) {
        unsafe {
            let sz = bitmap.GetSize();
            let is_fullscreen = sz.width >= sw * 0.92 && sz.height >= sh * 0.90;

            if is_fullscreen {
                let rect = D2D_RECT_F {
                    left: offset_x,
                    top: 0.0,
                    right: offset_x + sw,
                    bottom: sh,
                };
                d2d.draw_bitmap(bitmap, rect, 1.0);
            } else {
                // Centered aspect-fit card with rounded corners and dark shadow
                let aspect = if sz.height > 0.0 {
                    sz.width / sz.height
                } else {
                    1.6
                };
                let max_h = sh * 0.88;
                let max_w = sw * 0.88;
                let mut card_w = max_w.min(max_h * aspect);
                let mut card_h = card_w / aspect;
                if card_h > max_h {
                    card_h = max_h;
                    card_w = card_h * aspect;
                }

                let card_left = offset_x + (sw - card_w) * 0.5;
                let card_top = (sh - card_h) * 0.5;
                let card_rect = D2D_RECT_F {
                    left: card_left,
                    top: card_top,
                    right: card_left + card_w,
                    bottom: card_top + card_h,
                };

                // Ambient shadow behind non-maximized window
                d2d.fill_rounded_rect(
                    D2D_RECT_F {
                        left: card_left - 8.0,
                        top: card_top - 6.0,
                        right: card_left + card_w + 8.0,
                        bottom: card_top + card_h + 10.0,
                    },
                    18.0,
                    D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.40,
                    },
                );

                d2d.fill_rounded_bitmap(bitmap, card_rect, 14.0, 1.0);
                d2d.draw_rounded_rect(
                    card_rect,
                    14.0,
                    D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 0.15,
                    },
                    1.5,
                );
            }
        }
    }
}

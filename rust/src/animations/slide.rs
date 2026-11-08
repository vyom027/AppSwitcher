use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::ID2D1Bitmap;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;

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

        let curr_rect = D2D_RECT_F {
            left: curr_x,
            top: 0.0,
            right: curr_x + sw,
            bottom: sh,
        };

        let next_rect = D2D_RECT_F {
            left: next_x,
            top: 0.0,
            right: next_x + sw,
            bottom: sh,
        };

        d2d.draw_bitmap(curr_bitmap, curr_rect, 1.0);
        d2d.draw_bitmap(next_bitmap, next_rect, 1.0);
    }
}


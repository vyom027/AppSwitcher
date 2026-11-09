use crate::render::d2d::D2DContext;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
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
        // 100% 1:1 seamless full-frame slide — zero shrinking, zero scaling difference
        let rect = D2D_RECT_F {
            left: offset_x,
            top: 0.0,
            right: offset_x + sw,
            bottom: sh,
        };
        d2d.draw_bitmap(bitmap, rect, 1.0);
    }
}

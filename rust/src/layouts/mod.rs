pub mod coverflow;
pub mod dock;
pub mod fan;
pub mod grid;
pub mod hero;
pub mod row;
pub mod wallet;

use crate::render::d2d::D2DContext;
use crate::window::capture::ImageBuffer;
use crate::window::enumerator::WindowInfo;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

pub trait PickerLayout {
    fn update(&mut self, focus: f32, selected: usize);
    fn render(
        &self,
        d2d: &D2DContext,
        windows: &[WindowInfo],
        thumbnails: &[Option<ImageBuffer>],
        accent: D2D1_COLOR_F,
        sw: f32,
        sh: f32,
    );
    fn hit_test(&self, px: f32, py: f32, count: usize, sw: f32, sh: f32) -> Option<usize>;
}


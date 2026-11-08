#[derive(Debug, Clone, Copy)]
pub struct Point2D {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Quad2D {
    pub p0: Point2D, // Top-Left
    pub p1: Point2D, // Top-Right
    pub p2: Point2D, // Bottom-Right
    pub p3: Point2D, // Bottom-Left
}

/// 3x3 Projective Homography Matrix
#[derive(Debug, Clone, Copy)]
pub struct Matrix3x3 {
    pub m: [[f32; 3]; 3],
}

impl Matrix3x3 {
    pub fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
        }
    }

    /// Map a point (x, y) through the 3x3 projective transformation
    pub fn transform_point(&self, p: Point2D) -> Point2D {
        let x = p.x;
        let y = p.y;
        let w = self.m[2][0] * x + self.m[2][1] * y + self.m[2][2];
        let inv_w = if w.abs() > 1e-7 { 1.0 / w } else { 1.0 };

        let xp = (self.m[0][0] * x + self.m[0][1] * y + self.m[0][2]) * inv_w;
        let yp = (self.m[1][0] * x + self.m[1][1] * y + self.m[1][2]) * inv_w;

        Point2D { x: xp, y: yp }
    }

    /// Calculate closed-form Homography Matrix mapping unit square (0,0)-(1,1) to an arbitrary quad
    pub fn square_to_quad(q: &Quad2D) -> Self {
        let dx1 = q.p1.x - q.p2.x;
        let dx2 = q.p3.x - q.p2.x;
        let sx = q.p0.x - q.p1.x + q.p2.x - q.p3.x;

        let dy1 = q.p1.y - q.p2.y;
        let dy2 = q.p3.y - q.p2.y;
        let sy = q.p0.y - q.p1.y + q.p2.y - q.p3.y;

        if sx.abs() < 1e-7 && sy.abs() < 1e-7 {
            // Affine case
            Self {
                m: [
                    [q.p1.x - q.p0.x, q.p3.x - q.p0.x, q.p0.x],
                    [q.p1.y - q.p0.y, q.p3.y - q.p0.y, q.p0.y],
                    [0.0, 0.0, 1.0],
                ],
            }
        } else {
            let det = dx1 * dy2 - dx2 * dy1;
            let inv_det = if det.abs() > 1e-7 { 1.0 / det } else { 1.0 };

            let g = (sx * dy2 - sy * dx2) * inv_det;
            let h = (dx1 * sy - dy1 * sx) * inv_det;

            let a = q.p1.x - q.p0.x + g * q.p1.x;
            let b = q.p3.x - q.p0.x + h * q.p3.x;
            let c = q.p0.x;

            let d = q.p1.y - q.p0.y + g * q.p1.y;
            let e = q.p3.y - q.p0.y + h * q.p3.y;
            let f = q.p0.y;

            Self {
                m: [
                    [a, b, c],
                    [d, e, f],
                    [g, h, 1.0],
                ],
            }
        }
    }
}


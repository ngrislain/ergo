//! One physical-scale camera/frustum/ray conversion for drawing and picking.
use ergo_world::Aabb;
use glam::{DMat4, DVec3, DVec4};

pub const DISPLAY_SCALE: DVec3 = DVec3::new(
    1.,
    ergo_world::physical::BRICK_HEIGHT_M / ergo_world::physical::STUD_PITCH_M,
    1.,
);

#[derive(Clone)]
pub struct Visibility {
    pub planes: [[f64; 4]; 6],
    pub origin: DVec3,
    range: Option<(DVec3, f64)>,
}
impl Visibility {
    /// Matrix maps origin-relative construction coordinates to clip space.
    pub fn new(matrix: DMat4, origin: DVec3) -> Self {
        let rows = matrix.transpose().to_cols_array_2d().map(DVec4::from_array);
        Self {
            origin,
            range: None,
            planes: [
                rows[3] + rows[0],
                rows[3] - rows[0],
                rows[3] + rows[1],
                rows[3] - rows[1],
                rows[3] + rows[2],
                rows[3] - rows[2],
            ]
            .map(|p| p.to_array()),
        }
    }
    pub fn contains(&self, bounds: Aabb) -> bool {
        let center = bounds.center() - self.origin;
        let half = bounds.size() * 0.5;
        self.planes.iter().all(|p| {
            let normal = DVec3::new(p[0], p[1], p[2]);
            normal.dot(center) + p[3] + normal.abs().dot(half) >= 0.
        }) && self.range.is_none_or(|(eye, far)| {
            ((eye - eye.clamp(bounds.min, bounds.max)) * DISPLAY_SCALE).length() <= far
        })
    }
}

pub struct Camera {
    pub eye: DVec3,
    pub origin: DVec3,
    pub view_projection: DMat4,
    pub projection_pixels: f64,
    pub visibility: Visibility,
}
impl Camera {
    pub fn new(eye: DVec3, target: DVec3, width: f64, height: f64, fov: f64, far: f64) -> Self {
        let origin = (target / 1024.).floor() * 1024.;
        let projection =
            DMat4::perspective_rh_gl(fov.to_radians(), width.max(1.) / height.max(1.), 0.1, far);
        let view_projection = projection
            * DMat4::look_at_rh(
                (eye - origin) * DISPLAY_SCALE,
                (target - origin) * DISPLAY_SCALE,
                DVec3::Y,
            );
        let mut visibility =
            Visibility::new(view_projection * DMat4::from_scale(DISPLAY_SCALE), origin);
        visibility.range = Some((eye, far));
        Self {
            eye,
            origin,
            view_projection,
            projection_pixels: height.max(1.) / (2. * (fov.to_radians() * 0.5).tan()),
            visibility,
        }
    }
    pub fn ray(&self, x: f64, y: f64) -> (DVec3, DVec3) {
        let inverse = (self.view_projection * DMat4::from_scale(DISPLAY_SCALE)).inverse();
        let near = inverse.project_point3(DVec3::new(x, y, -1.));
        let far = inverse.project_point3(DVec3::new(x, y, 1.));
        (near + self.origin, (far - near).normalize())
    }
}

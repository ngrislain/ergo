//! Camera location is separate from the world's fixed geographic reference.
use ergo_world::{BlockCoord, GeoReference, coordinates};
use glam::DVec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraLocation {
    pub gps: [f64; 2],
    pub position: [f64; 3],
    pub block: [i64; 3],
}
impl CameraLocation {
    pub fn new(geo: GeoReference, eye: DVec3) -> Self {
        Self {
            gps: geo.to_gps(eye),
            position: coordinates::public(eye).to_array(),
            block: BlockCoord::at(eye).into(),
        }
    }
    pub fn gps_label(self) -> String {
        format!(
            "{:.7}° {}, {:.7}° {}",
            self.gps[0].abs(),
            if self.gps[0] < 0. { "S" } else { "N" },
            self.gps[1].abs(),
            if self.gps[1] < 0. { "W" } else { "E" }
        )
    }
    pub fn cell_label(self) -> String {
        format!("{}, {}, {}", self.block[0], self.block[1], self.block[2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_motion_changes_gps_without_moving_the_reference() {
        let geo = GeoReference::default();
        let start = CameraLocation::new(geo, DVec3::ZERO);
        let moved = CameraLocation::new(geo, coordinates::internal(DVec3::new(1024., 2048., 12.)));
        assert!((start.gps[0] - geo.latitude).abs() < 1e-10);
        assert!((start.gps[1] - geo.longitude).abs() < 1e-10);
        assert!((moved.gps[0] - geo.latitude - 2. / 3600.).abs() < 1e-10);
        assert!((moved.gps[1] - geo.longitude - 1. / 3600.).abs() < 1e-10);
        assert_eq!(moved.position, [1024., 2048., 12.]);
        assert_eq!(moved.block, [1, 2, 0]);
    }
    #[test]
    fn display_uses_hemispheres_and_negative_storage_cells() {
        let geo = GeoReference {
            latitude: -33.,
            longitude: -70.,
            arcseconds_per_block: 1.,
        };
        let location = CameraLocation::new(geo, coordinates::internal(DVec3::new(-1., -1., 4.)));
        assert!(location.gps_label().contains("° S"));
        assert!(location.gps_label().contains("° W"));
        assert_eq!(location.cell_label(), "-1, -1, 0");
    }
}

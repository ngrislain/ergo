//! Camera location is separate from the world's fixed geographic reference.
use ergo_world::{BlockCoord, GeoReference, coordinates};
use glam::DVec3;

/// Heading-relative compass: internal -Z is geographic north. Pitch does not
/// change this plan-view bearing, including in walk/fly navigation.
pub fn north_arrow_degrees(eye: DVec3, target: DVec3) -> Option<f64> {
    let forward = target - eye;
    (forward.is_finite() && forward.x.hypot(forward.z) > 1e-12)
        .then(|| -forward.x.atan2(-forward.z).to_degrees())
}

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
    fn compass_tracks_cardinal_headings_and_ignores_pitch() {
        for (forward, angle) in [
            (DVec3::NEG_Z, 0.),
            (DVec3::X, -90.),
            (DVec3::Z, -180.),
            (DVec3::NEG_X, 90.),
        ] {
            assert!((north_arrow_degrees(DVec3::ZERO, forward).unwrap() - angle).abs() < 1e-10);
            assert!(
                (north_arrow_degrees(DVec3::ZERO, forward + DVec3::Y * 8.).unwrap() - angle).abs()
                    < 1e-10
            );
        }
        assert_eq!(north_arrow_degrees(DVec3::ZERO, DVec3::Y), None);
    }
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

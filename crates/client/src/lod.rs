//! Stud detail has its own projected-size transition, separate from body LOD.
pub const STUD_FLAT_PX: f64 = 1.25;
pub const STUD_FULL_PX: f64 = 6.0;
pub const GROUND_STUD_RADIUS: f64 = 94.0;
pub const GROUND_GRID_RADIUS: i64 = 96;
/// Twelve-percent hysteresis avoids rebuilding batches at a detail boundary.
pub fn body_level(pixels: f64, previous: Option<u8>) -> u8 {
    let thresholds = [24., 8., 2.];
    let mut level = previous
        .unwrap_or_else(|| thresholds.iter().filter(|&&t| pixels < t).count() as u8)
        .min(3);
    while level > 0 && pixels >= thresholds[level as usize - 1] * 1.12 {
        level -= 1;
    }
    while level < 3 && pixels < thresholds[level as usize] * 0.88 {
        level += 1;
    }
    level
}
fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
pub fn stud_geometry(pixels: f64) -> f64 {
    smoothstep(STUD_FLAT_PX, STUD_FULL_PX, pixels)
}
pub fn ground_geometry(distance: f64, projection_pixels: f64, range: f64) -> f64 {
    stud_geometry(projection_pixels * 0.6 / distance.max(0.1))
        * (1. - smoothstep(range * 0.6, range, distance))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn body_hysteresis_holds_at_boundaries_but_allows_large_camera_moves() {
        assert_eq!(body_level(24.1, Some(1)), 1);
        assert_eq!(body_level(23.9, Some(0)), 0);
        assert_eq!(body_level(28., Some(1)), 0);
        assert_eq!(body_level(20., Some(0)), 1);
        assert_eq!(body_level(1., Some(0)), 3);
        assert_eq!(body_level(100., Some(3)), 0);
    }
    #[test]
    fn stud_transition_is_continuous_monotonic_and_not_a_body_size_cutoff() {
        assert_eq!(stud_geometry(1.), 0.);
        assert_eq!(stud_geometry(8.), 1.);
        let values = (0..1000)
            .map(|i| stud_geometry(i as f64 / 100.))
            .collect::<Vec<_>>();
        assert!(
            values
                .windows(2)
                .all(|v| v[1] >= v[0] && v[1] - v[0] < 0.004)
        );
        let values = (1..6000)
            .map(|i| ground_geometry(i as f64 / 100., 600., GROUND_STUD_RADIUS))
            .collect::<Vec<_>>();
        assert!(
            values
                .windows(2)
                .all(|v| v[1] <= v[0] && v[0] - v[1] < 0.002)
        );
        assert_eq!(
            ground_geometry(GROUND_STUD_RADIUS, 600., GROUND_STUD_RADIUS),
            0.
        );
    }
}
